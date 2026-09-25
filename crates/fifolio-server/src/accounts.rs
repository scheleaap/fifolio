//! Create, read, update, delete and list accounts [SRV-007].
//!
//! An account is addressed by its broker and the broker's own id, which is its key. A Saxo id
//! such as `69900/1000000` carries a slash, so a client percent-encodes it as `%2F` in the path;
//! axum decodes the segment back.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fifolio_core::entities::Account;
use fifolio_core::storage::{Database, StorageError};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::problem::Problem;

/// An account on the wire, as sent and as returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AccountBody {
    #[schema(example = "Saxo")]
    pub broker: String,
    /// The broker's id for the account, without any per-currency suffix [DOM-003].
    #[schema(example = "69900/1000000")]
    pub id: String,
}

impl From<&Account> for AccountBody {
    fn from(account: &Account) -> Self {
        Self {
            broker: account.broker().to_owned(),
            id: account.id().to_owned(),
        }
    }
}

impl From<AccountBody> for Account {
    fn from(body: AccountBody) -> Self {
        Account::new(body.broker, body.id)
    }
}

pub(crate) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_accounts, create_account))
        .routes(routes!(get_account, rename_account, delete_account))
}

/// Every account, ordered by broker and id [SRV-007].
#[utoipa::path(
    get,
    path = "/accounts",
    tag = "accounts",
    responses((status = 200, description = "Every account", body = [AccountBody]))
)]
async fn list_accounts(
    State(database): State<Database>,
) -> Result<Json<Vec<AccountBody>>, Problem> {
    let accounts = database.accounts().list().await?;
    Ok(Json(accounts.iter().map(AccountBody::from).collect()))
}

/// Stores an account [SRV-007].
#[utoipa::path(
    post,
    path = "/accounts",
    tag = "accounts",
    request_body = AccountBody,
    responses(
        (status = 201, description = "The account, as stored", body = AccountBody),
        (status = 409, description = "The account already exists", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn create_account(
    State(database): State<Database>,
    Json(body): Json<AccountBody>,
) -> Result<(StatusCode, Json<AccountBody>), Problem> {
    let account = Account::from(body);
    database.accounts().insert(&account).await?;
    Ok((StatusCode::CREATED, Json(AccountBody::from(&account))))
}

/// One account [SRV-007].
#[utoipa::path(
    get,
    path = "/accounts/{broker}/{id}",
    tag = "accounts",
    params(
        ("broker" = String, Path, description = "The broker"),
        ("id" = String, Path, description = "The broker's id for the account, percent-encoded"),
    ),
    responses(
        (status = 200, description = "The account", body = AccountBody),
        (status = 404, description = "No such account", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn get_account(
    State(database): State<Database>,
    Path((broker, id)): Path<(String, String)>,
) -> Result<Json<AccountBody>, Problem> {
    database
        .accounts()
        .find(&broker, &id)
        .await?
        .map(|account| Json(AccountBody::from(&account)))
        .ok_or_else(|| StorageError::UnknownAccount { broker, id }.into())
}

/// Gives an account another broker and id, the only edit an account has [SRV-007].
///
/// Refused while anything refers to the account, as a deletion is: every stored record's
/// identity is scoped to the account it was imported into [DOM-024].
#[utoipa::path(
    put,
    path = "/accounts/{broker}/{id}",
    tag = "accounts",
    params(
        ("broker" = String, Path, description = "The broker"),
        ("id" = String, Path, description = "The broker's id for the account, percent-encoded"),
    ),
    request_body = AccountBody,
    responses(
        (status = 200, description = "The account under its new key", body = AccountBody),
        (status = 404, description = "No such account", body = Problem,
         content_type = "application/problem+json"),
        (status = 409, description = "The account is referenced, or the new key is taken",
         body = Problem, content_type = "application/problem+json"),
    )
)]
async fn rename_account(
    State(database): State<Database>,
    Path((broker, id)): Path<(String, String)>,
    Json(body): Json<AccountBody>,
) -> Result<Json<AccountBody>, Problem> {
    let renamed = Account::from(body);
    database
        .accounts()
        .rename(&Account::new(broker, id), &renamed)
        .await?;
    Ok(Json(AccountBody::from(&renamed)))
}

/// Deletes an account, refused while any source record references it [SRV-008].
#[utoipa::path(
    delete,
    path = "/accounts/{broker}/{id}",
    tag = "accounts",
    params(
        ("broker" = String, Path, description = "The broker"),
        ("id" = String, Path, description = "The broker's id for the account, percent-encoded"),
    ),
    responses(
        (status = 204, description = "The account is deleted"),
        (status = 404, description = "No such account", body = Problem,
         content_type = "application/problem+json"),
        (status = 409, description = "The account is referenced", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn delete_account(
    State(database): State<Database>,
    Path((broker, id)): Path<(String, String)>,
) -> Result<StatusCode, Problem> {
    database
        .accounts()
        .delete(&Account::new(broker, id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
