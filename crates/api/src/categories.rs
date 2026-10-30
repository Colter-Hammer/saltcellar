//! Settings › Categories: groups and their categories, and the starter set
//! (docs/spec/m1-ledger-import.md#categories).

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use domain::categories::{GroupKind, GroupOrder, Name};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{Body, ErrorEnvelope, Id};
use crate::{ApiError, AppState};

/// Whether a group's categories are spending or money coming in.
#[derive(Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum CategoryGroupKind {
    Expense,
    Income,
}

impl From<GroupKind> for CategoryGroupKind {
    fn from(kind: GroupKind) -> Self {
        match kind {
            GroupKind::Expense => CategoryGroupKind::Expense,
            GroupKind::Income => CategoryGroupKind::Income,
        }
    }
}

impl From<CategoryGroupKind> for GroupKind {
    fn from(kind: CategoryGroupKind) -> Self {
        match kind {
            CategoryGroupKind::Expense => GroupKind::Expense,
            CategoryGroupKind::Income => GroupKind::Income,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct Category {
    id: i64,
    name: String,
    hidden: bool,
}

#[derive(Serialize, ToSchema)]
pub struct CategoryGroup {
    id: i64,
    name: String,
    kind: CategoryGroupKind,
    /// A hidden group hides its categories with it.
    hidden: bool,
    /// In order, hidden ones included.
    categories: Vec<Category>,
}

#[derive(Serialize, ToSchema)]
pub struct CategoryList {
    /// In order, hidden ones included.
    groups: Vec<CategoryGroup>,
}

/// `GET /api/v1/categories`: every group in order, each with its categories in order.
#[utoipa::path(
    get,
    path = "/api/v1/categories",
    operation_id = "getCategories",
    responses(
        (status = 200, description = "Every group and category, hidden ones included.", body = CategoryList),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<CategoryList>, ApiError> {
    let groups = db::category_groups(&state.pool).await?;
    Ok(Json(CategoryList {
        groups: groups
            .into_iter()
            .map(|group| CategoryGroup {
                id: group.id,
                name: group.name,
                kind: group.kind.into(),
                hidden: group.hidden,
                categories: group
                    .categories
                    .into_iter()
                    .map(|c| Category {
                        id: c.id,
                        name: c.name,
                        hidden: c.hidden,
                    })
                    .collect(),
            })
            .collect(),
    }))
}

/// `POST /api/v1/categories/starter`: writes the starter categories and the always-review payees.
#[utoipa::path(
    post,
    path = "/api/v1/categories/starter",
    operation_id = "useStarterCategories",
    responses(
        (status = 204, description = "The starter categories are written."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 409, description = "`categories_exist`: there's a group already.", body = ErrorEnvelope),
    ),
)]
pub async fn use_starter(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    db::use_starter_categories(&state.pool)
        .await
        .map_err(|error| Refused(error, Thing::Group))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct NewCategoryGroup {
    name: String,
    kind: CategoryGroupKind,
}

#[derive(Deserialize, ToSchema)]
pub struct NewCategory {
    group_id: i64,
    name: String,
}

/// What a change to a group or a category sets; a field left out stays as it is.
#[derive(Deserialize, ToSchema)]
pub struct CategoryChange {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    hidden: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct Created {
    id: i64,
}

/// `POST /api/v1/category-groups`: adds a group at the end.
#[utoipa::path(
    post,
    path = "/api/v1/category-groups",
    operation_id = "addCategoryGroup",
    request_body = NewCategoryGroup,
    responses(
        (status = 201, description = "The group is added.", body = Created),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: the name is empty, too long or taken.", body = ErrorEnvelope),
    ),
)]
pub async fn add_group(
    State(state): State<AppState>,
    Body(body): Body<NewCategoryGroup>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let name = valid_name(&body.name)?;
    let id = db::add_group(&state.pool, &name, body.kind.into())
        .await
        .map_err(|error| Refused(error, Thing::Group))
        .map_err(|error| error.name_taken(Thing::Group, &name))?;
    Ok((StatusCode::CREATED, Json(Created { id })))
}

/// `PATCH /api/v1/category-groups/{id}`: renames a group, hides it or shows it again.
#[utoipa::path(
    patch,
    path = "/api/v1/category-groups/{id}",
    operation_id = "changeCategoryGroup",
    params(("id" = i64, Path, description = "The group.")),
    request_body = CategoryChange,
    responses(
        (status = 204, description = "The group is changed."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such group.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: the name is empty, too long or taken.", body = ErrorEnvelope),
    ),
)]
pub async fn change_group(
    State(state): State<AppState>,
    Id(id): Id,
    Body(body): Body<CategoryChange>,
) -> Result<StatusCode, ApiError> {
    let name = body.name.as_deref().map(valid_name).transpose()?;
    db::update_group(&state.pool, id, name.as_ref(), body.hidden)
        .await
        .map_err(|error| Refused(error, Thing::Group))
        .map_err(|error| match &name {
            Some(name) => error.name_taken(Thing::Group, name),
            None => error.into(),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/v1/category-groups/{id}`: deletes an empty group.
#[utoipa::path(
    delete,
    path = "/api/v1/category-groups/{id}",
    operation_id = "deleteCategoryGroup",
    params(("id" = i64, Path, description = "The group.")),
    responses(
        (status = 204, description = "The group is deleted."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such group.", body = ErrorEnvelope),
        (status = 409, description = "`group_not_empty`: it still holds categories, hidden ones included.", body = ErrorEnvelope),
    ),
)]
pub async fn delete_group(
    State(state): State<AppState>,
    Id(id): Id,
) -> Result<StatusCode, ApiError> {
    db::delete_group(&state.pool, id)
        .await
        .map_err(|error| ApiError::from(Refused(error, Thing::Group)))?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/categories`: adds a category at the end of its group.
#[utoipa::path(
    post,
    path = "/api/v1/categories",
    operation_id = "addCategory",
    request_body = NewCategory,
    responses(
        (status = 201, description = "The category is added.", body = Created),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such group.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: the name is empty, too long or taken in the group.", body = ErrorEnvelope),
    ),
)]
pub async fn add_category(
    State(state): State<AppState>,
    Body(body): Body<NewCategory>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let name = valid_name(&body.name)?;
    let id = db::add_category(&state.pool, body.group_id, &name)
        .await
        .map_err(|error| Refused(error, Thing::Group))
        .map_err(|error| error.name_taken(Thing::Category, &name))?;
    Ok((StatusCode::CREATED, Json(Created { id })))
}

/// `PATCH /api/v1/categories/{id}`: renames a category, hides it or shows it again.
#[utoipa::path(
    patch,
    path = "/api/v1/categories/{id}",
    operation_id = "changeCategory",
    params(("id" = i64, Path, description = "The category.")),
    request_body = CategoryChange,
    responses(
        (status = 204, description = "The category is changed."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such category.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: the name is empty, too long or taken in its group.", body = ErrorEnvelope),
    ),
)]
pub async fn change_category(
    State(state): State<AppState>,
    Id(id): Id,
    Body(body): Body<CategoryChange>,
) -> Result<StatusCode, ApiError> {
    let name = body.name.as_deref().map(valid_name).transpose()?;
    db::update_category(&state.pool, id, name.as_ref(), body.hidden)
        .await
        .map_err(|error| Refused(error, Thing::Category))
        .map_err(|error| match &name {
            Some(name) => error.name_taken(Thing::Category, name),
            None => error.into(),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/v1/categories/{id}`: deletes a category. One that transactions or split lines use
/// answers `category_in_use` and can be hidden instead.
#[utoipa::path(
    delete,
    path = "/api/v1/categories/{id}",
    operation_id = "deleteCategory",
    params(("id" = i64, Path, description = "The category.")),
    responses(
        (status = 204, description = "The category is deleted."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such category.", body = ErrorEnvelope),
        (status = 409, description = "`category_in_use`: transactions or split lines use it.", body = ErrorEnvelope),
    ),
)]
pub async fn delete_category(
    State(state): State<AppState>,
    Id(id): Id,
) -> Result<StatusCode, ApiError> {
    db::delete_category(&state.pool, id)
        .await
        .map_err(|error| ApiError::from(Refused(error, Thing::Category)))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct CategoryGroupPlace {
    id: i64,
    /// The group's categories' ids, in order.
    categories: Vec<i64>,
}

#[derive(Deserialize, ToSchema)]
pub struct CategoryOrder {
    /// Every group, in order, each listing its categories: every group and every category exactly
    /// once, hidden ones included. A category listed under another group moves to it.
    groups: Vec<CategoryGroupPlace>,
}

/// `PUT /api/v1/categories/order`: puts every group and category in order.
#[utoipa::path(
    put,
    path = "/api/v1/categories/order",
    operation_id = "orderCategories",
    request_body = CategoryOrder,
    responses(
        (status = 204, description = "The order is saved."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 409, description = "`categories_changed`: the order doesn't list every group and category exactly once; `name_taken`: a moved category's name is taken in its new group.", body = ErrorEnvelope),
    ),
)]
pub async fn order(
    State(state): State<AppState>,
    Body(body): Body<CategoryOrder>,
) -> Result<StatusCode, ApiError> {
    let order: Vec<GroupOrder> = body
        .groups
        .into_iter()
        .map(|group| GroupOrder {
            group_id: group.id,
            category_ids: group.categories,
        })
        .collect();
    db::set_order(&state.pool, &order)
        .await
        .map_err(|error| ApiError::from(Refused(error, Thing::Category)))?;
    Ok(StatusCode::NO_CONTENT)
}

fn valid_name(typed: &str) -> Result<Name, ApiError> {
    Name::new(typed).map_err(|error| {
        let fields = BTreeMap::from([("name".to_owned(), error.to_string())]);
        ApiError::validation("Check the name.", fields)
    })
}

#[derive(Clone, Copy)]
enum Thing {
    Group,
    Category,
}

/// A refusal from the database, on its way to becoming an [`ApiError`]: a taken name becomes a
/// field error when the request had a name in it, and a `name_taken` conflict when it didn't.
struct Refused(db::CategoryError, Thing);

impl Refused {
    fn name_taken(self, thing: Thing, name: &Name) -> ApiError {
        if matches!(self.0, db::CategoryError::NameTaken) {
            let message = match thing {
                Thing::Group => format!("There's already a group called {name}."),
                Thing::Category => format!("This group already has a category called {name}."),
            };
            let fields = BTreeMap::from([("name".to_owned(), message)]);
            return ApiError::validation("Check the name.", fields);
        }
        self.into()
    }
}

impl From<Refused> for ApiError {
    fn from(Refused(error, thing): Refused) -> Self {
        match error {
            db::CategoryError::NotFound => ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                match thing {
                    Thing::Group => "There's no such group.",
                    Thing::Category => "There's no such category.",
                },
            ),
            db::CategoryError::NameTaken => ApiError::new(
                StatusCode::CONFLICT,
                "name_taken",
                "A category with that name is already in that group.",
            ),
            db::CategoryError::GroupNotEmpty => ApiError::new(
                StatusCode::CONFLICT,
                "group_not_empty",
                "Delete or move its categories first, hidden ones too.",
            ),
            db::CategoryError::NotEmpty => ApiError::new(
                StatusCode::CONFLICT,
                "categories_exist",
                "There are categories already: the starter set is for an empty list.",
            ),
            db::CategoryError::InUse => ApiError::new(
                StatusCode::CONFLICT,
                "category_in_use",
                "Transactions use this category, so it can't be deleted. Hide it instead.",
            ),
            db::CategoryError::Order(_) => ApiError::new(
                StatusCode::CONFLICT,
                "categories_changed",
                "The categories changed since this screen loaded. Reload and try again.",
            ),
            db::CategoryError::Db(error) => error.into(),
        }
    }
}
