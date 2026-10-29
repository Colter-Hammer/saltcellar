use domain::{Currency, TimeZone};

use crate::{Error, SqlitePool};

/// The household's settings that the whole app reads. It exists once setup is finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Household {
    pub currency: Currency,
    pub timezone: TimeZone,
}

/// The household, or `None` while setup isn't finished.
///
/// # Errors
/// If the database fails, or holds a currency or timezone the app no longer knows.
pub async fn household(pool: &SqlitePool) -> Result<Option<Household>, Error> {
    let row = sqlx::query!(
        "SELECT currency_code, timezone FROM household WHERE id = 1 AND setup_done_at IS NOT NULL"
    )
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let currency = row.currency_code.parse().map_err(|_| Error::Unreadable {
        what: "a currency",
        value: row.currency_code.clone(),
    })?;
    let timezone = row.timezone.parse().map_err(|_| Error::Unreadable {
        what: "a timezone",
        value: row.timezone.clone(),
    })?;
    Ok(Some(Household { currency, timezone }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::migrated;

    #[tokio::test]
    async fn there_is_none_before_setup() {
        let (_dir, pool) = migrated().await;
        assert_eq!(household(&pool).await.unwrap(), None);
        sqlx::query("INSERT INTO household (id, currency_code, timezone) VALUES (1, 'EUR', 'UTC')")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(household(&pool).await.unwrap(), None);
    }

    #[tokio::test]
    async fn reads_the_finished_household() {
        let (_dir, pool) = migrated().await;
        sqlx::query(
            "INSERT INTO household (id, currency_code, timezone, setup_done_at) \
             VALUES (1, 'EUR', 'Europe/Berlin', '2026-10-28T09:00:00.000Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            household(&pool).await.unwrap(),
            Some(Household {
                currency: "EUR".parse().unwrap(),
                timezone: "Europe/Berlin".parse().unwrap(),
            })
        );
    }

    #[tokio::test]
    async fn says_which_stored_value_it_cant_read() {
        for (currency, timezone, what) in [
            ("XXX", "UTC", "a currency"),
            ("EUR", "Nowhere/Special", "a timezone"),
        ] {
            let (_dir, pool) = migrated().await;
            sqlx::query(
                "INSERT INTO household (id, currency_code, timezone, setup_done_at) \
                 VALUES (1, ?, ?, '2026-10-28T09:00:00.000Z')",
            )
            .bind(currency)
            .bind(timezone)
            .execute(&pool)
            .await
            .unwrap();
            let error = household(&pool).await.unwrap_err();
            assert!(
                matches!(&error, Error::Unreadable { what: w, .. } if *w == what),
                "{error}"
            );
        }
    }
}
