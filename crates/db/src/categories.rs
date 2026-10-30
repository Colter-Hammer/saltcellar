//! Category groups, categories and the starter set (docs/spec/m1-ledger-import.md#categories).

use domain::categories::{
    ALWAYS_REVIEW_PAYEES, GroupKind, GroupOrder, Name, OrderError, STARTER_GROUPS, check_order,
};
use sqlx::SqliteConnection;

use crate::{Error, SqlitePool};

/// A group, with its categories in order. Hidden ones are included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub kind: GroupKind,
    pub hidden: bool,
    pub categories: Vec<Category>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub hidden: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CategoryError {
    #[error("there's no such group or category")]
    NotFound,
    #[error("the name is taken")]
    NameTaken,
    #[error("the group still holds categories")]
    GroupNotEmpty,
    #[error("there are categories already")]
    NotEmpty,
    #[error("transactions or split lines use the category")]
    InUse,
    #[error(transparent)]
    Order(#[from] OrderError),
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for CategoryError {
    fn from(error: sqlx::Error) -> Self {
        let unique = error
            .as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_unique_violation);
        if unique {
            CategoryError::NameTaken
        } else {
            CategoryError::Db(Error::Sqlx(error))
        }
    }
}

/// Every group in order, each with its categories in order.
///
/// # Errors
/// If the database fails or holds a group kind the app doesn't know.
pub async fn category_groups(pool: &SqlitePool) -> Result<Vec<Group>, Error> {
    let groups = sqlx::query!(
        r#"SELECT id AS "id!: i64", name, kind, hidden AS "hidden: bool"
           FROM category_groups ORDER BY sort_order, id"#
    )
    .fetch_all(pool)
    .await?;
    let categories = sqlx::query!(
        r#"SELECT id AS "id!: i64", group_id, name, hidden AS "hidden: bool"
           FROM categories ORDER BY sort_order, id"#
    )
    .fetch_all(pool)
    .await?;
    groups
        .into_iter()
        .map(|group| {
            let kind = group.kind.parse().map_err(|_| Error::Unreadable {
                what: "a category group kind",
                value: group.kind.clone(),
            })?;
            Ok(Group {
                id: group.id,
                name: group.name,
                kind,
                hidden: group.hidden,
                categories: categories
                    .iter()
                    .filter(|category| category.group_id == group.id)
                    .map(|category| Category {
                        id: category.id,
                        name: category.name.clone(),
                        hidden: category.hidden,
                    })
                    .collect(),
            })
        })
        .collect()
}

/// Writes the starter categories, in order, and seeds the always-review payees. A payee that
/// already exists keeps its review mode *(agent)*.
///
/// # Errors
/// [`CategoryError::NotEmpty`] if there's any group already, which leaves everything as it was;
/// otherwise only if the database fails.
pub async fn use_starter_categories(pool: &SqlitePool) -> Result<(), CategoryError> {
    let mut tx = pool.begin().await?;
    let existing = sqlx::query_scalar!(r#"SELECT count(*) AS "n!: i64" FROM category_groups"#)
        .fetch_one(&mut *tx)
        .await?;
    if existing > 0 {
        return Err(CategoryError::NotEmpty);
    }
    for (group_order, group) in (0_i64..).zip(STARTER_GROUPS) {
        let kind = group.kind.as_str();
        let group_id = sqlx::query_scalar!(
            r#"INSERT INTO category_groups (name, kind, sort_order) VALUES (?, ?, ?)
               RETURNING id AS "id!: i64""#,
            group.name,
            kind,
            group_order
        )
        .fetch_one(&mut *tx)
        .await?;
        for (order, name) in (0_i64..).zip(group.categories) {
            sqlx::query!(
                "INSERT INTO categories (group_id, name, sort_order) VALUES (?, ?, ?)",
                group_id,
                name,
                order
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    for name in ALWAYS_REVIEW_PAYEES {
        sqlx::query!(
            "INSERT INTO payees (name, review_mode) VALUES (?, 'always') ON CONFLICT (name) DO NOTHING",
            name
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Adds a group at the end.
///
/// # Errors
/// [`CategoryError::NameTaken`] if another group has the name; otherwise only if the database
/// fails.
pub async fn add_group(
    pool: &SqlitePool,
    name: &Name,
    kind: GroupKind,
) -> Result<i64, CategoryError> {
    let name = name.as_str();
    let kind = kind.as_str();
    let id = sqlx::query_scalar!(
        r#"INSERT INTO category_groups (name, kind, sort_order)
           VALUES (?, ?, (SELECT coalesce(max(sort_order) + 1, 0) FROM category_groups))
           RETURNING id AS "id!: i64""#,
        name,
        kind
    )
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Renames a group, hides it or shows it again; `None` leaves that part as it is.
///
/// # Errors
/// [`CategoryError::NotFound`] if there's no such group, [`CategoryError::NameTaken`] if another
/// group has the name; otherwise only if the database fails.
pub async fn update_group(
    pool: &SqlitePool,
    id: i64,
    name: Option<&Name>,
    hidden: Option<bool>,
) -> Result<(), CategoryError> {
    let name = name.map(Name::as_str);
    let updated = sqlx::query!(
        "UPDATE category_groups SET name = coalesce(?, name), hidden = coalesce(?, hidden) WHERE id = ?",
        name,
        hidden,
        id
    )
    .execute(pool)
    .await?;
    found(updated.rows_affected())
}

/// Deletes an empty group.
///
/// # Errors
/// [`CategoryError::NotFound`] if there's no such group, [`CategoryError::GroupNotEmpty`] while it
/// holds any category, hidden ones included; otherwise only if the database fails.
pub async fn delete_group(pool: &SqlitePool, id: i64) -> Result<(), CategoryError> {
    let mut tx = pool.begin().await?;
    let held = sqlx::query_scalar!(
        r#"SELECT count(*) AS "n!: i64" FROM categories WHERE group_id = ?"#,
        id
    )
    .fetch_one(&mut *tx)
    .await?;
    if held > 0 {
        return Err(CategoryError::GroupNotEmpty);
    }
    let deleted = sqlx::query!("DELETE FROM category_groups WHERE id = ?", id)
        .execute(&mut *tx)
        .await?;
    found(deleted.rows_affected())?;
    tx.commit().await?;
    Ok(())
}

/// Adds a category at the end of its group.
///
/// # Errors
/// [`CategoryError::NotFound`] if there's no such group, [`CategoryError::NameTaken`] if the group
/// has a category with the name; otherwise only if the database fails.
pub async fn add_category(
    pool: &SqlitePool,
    group_id: i64,
    name: &Name,
) -> Result<i64, CategoryError> {
    let name = name.as_str();
    let mut tx = pool.begin().await?;
    let group = sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM category_groups WHERE id = ?"#,
        group_id
    )
    .fetch_optional(&mut *tx)
    .await?;
    if group.is_none() {
        return Err(CategoryError::NotFound);
    }
    let id = sqlx::query_scalar!(
        r#"INSERT INTO categories (group_id, name, sort_order)
           VALUES (?1, ?2, (SELECT coalesce(max(sort_order) + 1, 0) FROM categories WHERE group_id = ?1))
           RETURNING id AS "id!: i64""#,
        group_id,
        name
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Renames a category, hides it or shows it again; `None` leaves that part as it is.
///
/// # Errors
/// [`CategoryError::NotFound`] if there's no such category, [`CategoryError::NameTaken`] if its
/// group has another category with the name; otherwise only if the database fails.
pub async fn update_category(
    pool: &SqlitePool,
    id: i64,
    name: Option<&Name>,
    hidden: Option<bool>,
) -> Result<(), CategoryError> {
    let name = name.map(Name::as_str);
    let updated = sqlx::query!(
        "UPDATE categories SET name = coalesce(?, name), hidden = coalesce(?, hidden) WHERE id = ?",
        name,
        hidden,
        id
    )
    .execute(pool)
    .await?;
    found(updated.rows_affected())
}

/// Deletes a category. One that transactions or split lines use is refused and can only be hidden
/// (rules join the check in milestone 3).
///
/// # Errors
/// [`CategoryError::InUse`] while anything uses it, [`CategoryError::NotFound`] if there's no such
/// category; otherwise only if the database fails.
pub async fn delete_category(pool: &SqlitePool, id: i64) -> Result<(), CategoryError> {
    let mut tx = pool.begin().await?;
    let in_use = sqlx::query_scalar!(
        r#"SELECT (EXISTS (SELECT 1 FROM transactions WHERE category_id = ?1)
                OR EXISTS (SELECT 1 FROM transaction_splits WHERE category_id = ?1)) AS "in_use!: bool""#,
        id
    )
    .fetch_one(&mut *tx)
    .await?;
    if in_use {
        return Err(CategoryError::InUse);
    }
    let deleted = sqlx::query!("DELETE FROM categories WHERE id = ?", id)
        .execute(&mut *tx)
        .await?;
    found(deleted.rows_affected())?;
    tx.commit().await?;
    Ok(())
}

/// Puts every group and category in the given order, moving categories between groups as listed.
///
/// # Errors
/// [`CategoryError::Order`] if `order` isn't a rearrangement of every group and category there is,
/// [`CategoryError::NameTaken`] if a moved category's name is taken in its new group; either
/// leaves everything as it was. Otherwise only if the database fails.
pub async fn set_order(pool: &SqlitePool, order: &[GroupOrder]) -> Result<(), CategoryError> {
    let mut tx = pool.begin().await?;
    check_order(&current_order(&mut tx).await?, order)?;
    for (group_order, group) in (0_i64..).zip(order) {
        sqlx::query!(
            "UPDATE category_groups SET sort_order = ? WHERE id = ?",
            group_order,
            group.group_id
        )
        .execute(&mut *tx)
        .await?;
        for (category_order, id) in (0_i64..).zip(&group.category_ids) {
            sqlx::query!(
                "UPDATE categories SET group_id = ?, sort_order = ? WHERE id = ?",
                group.group_id,
                category_order,
                id
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

async fn current_order(conn: &mut SqliteConnection) -> Result<Vec<GroupOrder>, sqlx::Error> {
    let groups = sqlx::query_scalar!(r#"SELECT id AS "id!: i64" FROM category_groups"#)
        .fetch_all(&mut *conn)
        .await?;
    let categories = sqlx::query!(r#"SELECT id AS "id!: i64", group_id FROM categories"#)
        .fetch_all(&mut *conn)
        .await?;
    Ok(groups
        .into_iter()
        .map(|group_id| GroupOrder {
            group_id,
            category_ids: categories
                .iter()
                .filter(|c| c.group_id == group_id)
                .map(|c| c.id)
                .collect(),
        })
        .collect())
}

fn found(rows: u64) -> Result<(), CategoryError> {
    if rows == 0 {
        Err(CategoryError::NotFound)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::migrated;

    fn name(text: &str) -> Name {
        Name::new(text).unwrap()
    }

    /// Each group as `(name, [category names])`, hidden ones marked with a trailing `*`.
    async fn outline(pool: &SqlitePool) -> Vec<(String, Vec<String>)> {
        let mark = |name: &str, hidden: bool| {
            if hidden {
                format!("{name}*")
            } else {
                name.to_owned()
            }
        };
        category_groups(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|g| {
                (
                    mark(&g.name, g.hidden),
                    g.categories
                        .iter()
                        .map(|c| mark(&c.name, c.hidden))
                        .collect(),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn there_are_none_to_start_with() {
        let (_dir, pool) = migrated().await;
        assert_eq!(category_groups(&pool).await.unwrap(), []);
    }

    #[tokio::test]
    async fn the_starter_set_is_written_in_order_with_its_payees() {
        let (_dir, pool) = migrated().await;
        use_starter_categories(&pool).await.unwrap();
        let groups = category_groups(&pool).await.unwrap();
        assert_eq!(groups.len(), 12);
        for (group, starter) in groups.iter().zip(STARTER_GROUPS) {
            assert_eq!(group.name, starter.name);
            assert_eq!(group.kind, starter.kind);
            assert!(!group.hidden);
            let names: Vec<&str> = group.categories.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(names, starter.categories);
        }
        let income = groups.last().unwrap();
        assert_eq!(
            (income.name.as_str(), income.kind),
            ("Income", GroupKind::Income)
        );

        let payees: Vec<(String, String)> =
            sqlx::query_as("SELECT name, review_mode FROM payees ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        let expected: Vec<(String, String)> = ALWAYS_REVIEW_PAYEES
            .iter()
            .map(|p| ((*p).to_owned(), "always".to_owned()))
            .collect();
        assert_eq!(payees, expected);
    }

    #[tokio::test]
    async fn the_starter_set_is_refused_once_there_is_a_group() {
        let (_dir, pool) = migrated().await;
        add_group(&pool, &name("Bills"), GroupKind::Expense)
            .await
            .unwrap();
        let error = use_starter_categories(&pool).await.unwrap_err();
        assert!(matches!(error, CategoryError::NotEmpty), "{error}");
        assert_eq!(outline(&pool).await, [("Bills".to_owned(), vec![])]);
        let payees: i64 = sqlx::query_scalar("SELECT count(*) FROM payees")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(payees, 0);
    }

    #[tokio::test]
    async fn a_payee_that_exists_keeps_its_review_mode() {
        let (_dir, pool) = migrated().await;
        sqlx::query("INSERT INTO payees (name, review_mode) VALUES ('Venmo', 'never')")
            .execute(&pool)
            .await
            .unwrap();
        use_starter_categories(&pool).await.unwrap();
        let mode: String =
            sqlx::query_scalar("SELECT review_mode FROM payees WHERE name = 'Venmo'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(mode, "never");
    }

    #[tokio::test]
    async fn groups_and_categories_are_added_at_the_end() {
        let (_dir, pool) = migrated().await;
        let bills = add_group(&pool, &name("Bills"), GroupKind::Expense)
            .await
            .unwrap();
        let pay = add_group(&pool, &name("Pay"), GroupKind::Income)
            .await
            .unwrap();
        add_category(&pool, bills, &name("Water")).await.unwrap();
        add_category(&pool, bills, &name("Power")).await.unwrap();
        add_category(&pool, pay, &name("Water")).await.unwrap();
        assert_eq!(
            outline(&pool).await,
            [
                (
                    "Bills".to_owned(),
                    vec!["Water".to_owned(), "Power".to_owned()]
                ),
                ("Pay".to_owned(), vec!["Water".to_owned()]),
            ]
        );
        assert_eq!(
            category_groups(&pool).await.unwrap()[1].kind,
            GroupKind::Income
        );
    }

    #[tokio::test]
    async fn names_are_unique_among_groups_and_within_a_group() {
        let (_dir, pool) = migrated().await;
        let bills = add_group(&pool, &name("Bills"), GroupKind::Expense)
            .await
            .unwrap();
        let fun = add_group(&pool, &name("Fun"), GroupKind::Expense)
            .await
            .unwrap();
        add_category(&pool, bills, &name("Water")).await.unwrap();
        let power = add_category(&pool, bills, &name("Power")).await.unwrap();

        let taken = [
            add_group(&pool, &name("Bills"), GroupKind::Income)
                .await
                .map(|_| ()),
            update_group(&pool, fun, Some(&name("Bills")), None).await,
            add_category(&pool, bills, &name("Water")).await.map(|_| ()),
            update_category(&pool, power, Some(&name("Water")), None).await,
        ];
        for result in taken {
            assert!(
                matches!(result, Err(CategoryError::NameTaken)),
                "{result:?}"
            );
        }
    }

    #[tokio::test]
    async fn renaming_and_hiding_change_only_what_they_say() {
        let (_dir, pool) = migrated().await;
        let bills = add_group(&pool, &name("Bills"), GroupKind::Expense)
            .await
            .unwrap();
        let water = add_category(&pool, bills, &name("Water")).await.unwrap();
        update_group(&pool, bills, Some(&name("Home")), None)
            .await
            .unwrap();
        update_category(&pool, water, None, Some(true))
            .await
            .unwrap();
        assert_eq!(
            outline(&pool).await,
            [("Home".to_owned(), vec!["Water*".to_owned()])]
        );
        update_group(&pool, bills, None, Some(true)).await.unwrap();
        update_category(&pool, water, Some(&name("Sewer")), Some(false))
            .await
            .unwrap();
        assert_eq!(
            outline(&pool).await,
            [("Home*".to_owned(), vec!["Sewer".to_owned()])]
        );
    }

    #[tokio::test]
    async fn changing_or_deleting_what_isnt_there_is_not_found() {
        let (_dir, pool) = migrated().await;
        let results = [
            update_group(&pool, 9, Some(&name("X")), None).await,
            update_category(&pool, 9, None, Some(true)).await,
            delete_group(&pool, 9).await,
            delete_category(&pool, 9).await,
            add_category(&pool, 9, &name("X")).await.map(|_| ()),
        ];
        for result in results {
            assert!(matches!(result, Err(CategoryError::NotFound)), "{result:?}");
        }
    }

    #[tokio::test]
    async fn a_group_cant_be_deleted_while_it_holds_categories() {
        let (_dir, pool) = migrated().await;
        let bills = add_group(&pool, &name("Bills"), GroupKind::Expense)
            .await
            .unwrap();
        let water = add_category(&pool, bills, &name("Water")).await.unwrap();
        update_category(&pool, water, None, Some(true))
            .await
            .unwrap();
        let error = delete_group(&pool, bills).await.unwrap_err();
        assert!(matches!(error, CategoryError::GroupNotEmpty), "{error}");

        delete_category(&pool, water).await.unwrap();
        delete_group(&pool, bills).await.unwrap();
        assert_eq!(outline(&pool).await, []);
    }

    #[tokio::test]
    async fn the_schema_itself_refuses_deleting_a_group_that_holds_categories() {
        let (_dir, pool) = migrated().await;
        let bills = add_group(&pool, &name("Bills"), GroupKind::Expense)
            .await
            .unwrap();
        add_category(&pool, bills, &name("Water")).await.unwrap();
        let direct = sqlx::query("DELETE FROM category_groups WHERE id = ?")
            .bind(bills)
            .execute(&pool)
            .await;
        assert!(direct.is_err());
        let orphan = sqlx::query("INSERT INTO categories (group_id, name) VALUES (99, 'Lost')")
            .execute(&pool)
            .await;
        assert!(orphan.is_err());
        let no_group = sqlx::query("INSERT INTO categories (name) VALUES ('Lost')")
            .execute(&pool)
            .await;
        assert!(no_group.is_err());
        let bad_kind =
            sqlx::query("INSERT INTO category_groups (name, kind) VALUES ('Odd', 'savings')")
                .execute(&pool)
                .await;
        assert!(bad_kind.is_err());
    }

    #[tokio::test]
    async fn the_order_is_saved_and_moves_categories_between_groups() {
        let (_dir, pool) = migrated().await;
        let bills = add_group(&pool, &name("Bills"), GroupKind::Expense)
            .await
            .unwrap();
        let fun = add_group(&pool, &name("Fun"), GroupKind::Expense)
            .await
            .unwrap();
        let water = add_category(&pool, bills, &name("Water")).await.unwrap();
        let power = add_category(&pool, bills, &name("Power")).await.unwrap();
        let games = add_category(&pool, fun, &name("Games")).await.unwrap();

        let order = [
            GroupOrder {
                group_id: fun,
                category_ids: vec![games, water],
            },
            GroupOrder {
                group_id: bills,
                category_ids: vec![power],
            },
        ];
        set_order(&pool, &order).await.unwrap();
        assert_eq!(
            outline(&pool).await,
            [
                (
                    "Fun".to_owned(),
                    vec!["Games".to_owned(), "Water".to_owned()]
                ),
                ("Bills".to_owned(), vec!["Power".to_owned()]),
            ]
        );
    }

    #[tokio::test]
    async fn an_order_that_puts_a_category_in_two_groups_changes_nothing() {
        let (_dir, pool) = migrated().await;
        let bills = add_group(&pool, &name("Bills"), GroupKind::Expense)
            .await
            .unwrap();
        let fun = add_group(&pool, &name("Fun"), GroupKind::Expense)
            .await
            .unwrap();
        let water = add_category(&pool, bills, &name("Water")).await.unwrap();
        let before = outline(&pool).await;
        let order = [
            GroupOrder {
                group_id: fun,
                category_ids: vec![water],
            },
            GroupOrder {
                group_id: bills,
                category_ids: vec![water],
            },
        ];
        let error = set_order(&pool, &order).await.unwrap_err();
        assert!(
            matches!(error, CategoryError::Order(OrderError::CategoryTwice(id)) if id == water),
            "{error}"
        );
        assert_eq!(outline(&pool).await, before);
    }

    #[tokio::test]
    async fn moving_a_category_onto_a_name_taken_in_its_new_group_changes_nothing() {
        let (_dir, pool) = migrated().await;
        let bills = add_group(&pool, &name("Bills"), GroupKind::Expense)
            .await
            .unwrap();
        let fun = add_group(&pool, &name("Fun"), GroupKind::Expense)
            .await
            .unwrap();
        let water = add_category(&pool, bills, &name("Water")).await.unwrap();
        let other = add_category(&pool, fun, &name("Water")).await.unwrap();
        let before = outline(&pool).await;
        let order = [
            GroupOrder {
                group_id: bills,
                category_ids: vec![],
            },
            GroupOrder {
                group_id: fun,
                category_ids: vec![other, water],
            },
        ];
        let error = set_order(&pool, &order).await.unwrap_err();
        assert!(matches!(error, CategoryError::NameTaken), "{error}");
        assert_eq!(outline(&pool).await, before);
    }

    #[tokio::test]
    async fn an_unknown_kind_in_the_database_is_unreadable() {
        let (_dir, pool) = migrated().await;
        // Only a database edited by hand could hold one: the CHECK refuses it, so drop the CHECK.
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO category_groups (name, kind) VALUES ('Odd', 'savings')")
            .execute(&mut *conn)
            .await
            .unwrap();
        drop(conn);
        let error = category_groups(&pool).await.unwrap_err();
        assert!(matches!(error, Error::Unreadable { .. }), "{error}");
    }
}
