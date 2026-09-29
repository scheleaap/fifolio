//! HTTP API over `fifolio-core`; the only process that opens the database [ARC-003].
//!
//! The binary is `main.rs`; everything it does is here, so the arguments, the listening address
//! and the router are testable without spawning a process.
//!
//! The OpenAPI document is assembled from the handlers' own `#[utoipa::path]` annotations by
//! [`OpenApiRouter`], so the routes served and the routes documented are one list and cannot
//! drift apart. [`openapi`] and `GET /openapi.json` both read that list, which is what makes
//! `fifolio-server openapi` print the same document the running server returns [SRV-005,
//! SRV-006].
//!
//! Every error response is a problem document; see [`problem`] [ARC-020].

pub mod accounts;
pub mod imports;
pub mod manual_entries;
pub mod problem;
pub mod securities;
pub mod source_records;
pub mod transactions;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{FromRef, State};
use axum::middleware::map_response;
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use fifolio_core::storage::{DEFAULT_DATABASE_PATH, Database, StorageError};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

/// The port served when `--port` is absent [SRV-002].
pub const DEFAULT_PORT: u16 = 8000;

/// `fifolio-server [--port N] [--database PATH] [openapi]`.
#[derive(Debug, Parser)]
#[command(name = "fifolio-server", version, about)]
pub struct Args {
    /// The loopback port to listen on [SRV-003].
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,
    /// The SQLite file, created on first run [SRV-004].
    #[arg(long, default_value = DEFAULT_DATABASE_PATH)]
    pub database: PathBuf,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Print the OpenAPI spec and exit, without opening the database or starting the server.
    Openapi,
}

/// Why the server did not start or stopped serving.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("cannot open the database {}: {source}", path.display())]
    Database {
        path: PathBuf,
        #[source]
        source: StorageError,
    },
    #[error("cannot listen on {address}: {source}")]
    Bind {
        address: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("the server stopped: {0}")]
    Serve(#[source] std::io::Error),
}

/// The only address the server ever binds: the data is financial and unprotected, so there is
/// no argument that widens it [ARC-022].
#[must_use]
pub fn listen_address(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

#[derive(OpenApi)]
#[openapi(info(title = "fifolio-server"), components(schemas(problem::Problem)))]
struct ApiDoc;

/// What a handler can extract: the spec it serves, and the database it reaches storage
/// through, the one handle this process holds [ARC-003].
#[derive(Clone, FromRef)]
struct AppState {
    spec: Arc<utoipa::openapi::OpenApi>,
    database: Database,
}

/// Every documented route. A handler added here is served and documented at once.
fn api() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(openapi_json))
        .merge(accounts::routes())
        .merge(imports::routes())
        .merge(manual_entries::routes())
        .merge(securities::routes())
        .merge(source_records::routes())
        .merge(transactions::routes())
}

/// The server's OpenAPI document [SRV-005].
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    api().split_for_parts().1
}

/// The HTTP surface over `database`.
///
/// The problem layer goes on last so it sees every response, the fallback's 404 and a route's
/// 405 included [ARC-020].
pub fn router(database: Database) -> Router {
    let (router, spec) = api().split_for_parts();
    router
        .with_state(AppState {
            spec: Arc::new(spec),
            database,
        })
        .layer(map_response(problem::problem_for_bare_errors))
}

/// The server's OpenAPI document [SRV-006].
#[utoipa::path(
    get,
    path = "/openapi.json",
    tag = "meta",
    responses((status = 200, description = "This server's OpenAPI document", content_type = "application/json"))
)]
async fn openapi_json(
    State(spec): State<Arc<utoipa::openapi::OpenApi>>,
) -> Json<utoipa::openapi::OpenApi> {
    Json(spec.as_ref().clone())
}

/// Opens the database, then listens on loopback until interrupted.
///
/// The database is opened before the socket is bound, so a server that answers is one whose
/// schema is current, and a database that cannot be opened never leaves a port listening. The
/// handle is held for the life of the process: this process is the one that opens the file
/// [ARC-003].
pub async fn serve(port: u16, database: PathBuf) -> Result<(), ServeError> {
    let db = Database::open(&database)
        .await
        .map_err(|source| ServeError::Database {
            path: database.clone(),
            source,
        })?;
    let address = listen_address(port);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|source| ServeError::Bind { address, source })?;
    tracing::info!(%address, database = %database.display(), "fifolio-server listening");
    let served = axum::serve(listener, router(db.clone()))
        .with_graceful_shutdown(async {
            // An error here means no signal handler could be installed; the server then runs
            // until killed, which is what it would do without graceful shutdown at all.
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(ServeError::Serve);
    db.close().await;
    served
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Args {
        Args::try_parse_from(argv).expect("the arguments parse")
    }

    /// Without arguments the server listens on port 8000 over `./fifolio.db` [SRV-002, SRV-004].
    #[test]
    fn defaults_are_port_8000_and_the_working_directory_database() {
        let args = parse(&["fifolio-server"]);
        assert_eq!(args.port, 8000);
        assert_eq!(args.database, PathBuf::from("./fifolio.db"));
        assert_eq!(args.command, None);
    }

    /// `--port` and `--database` override the defaults [SRV-003, SRV-004].
    #[test]
    fn port_and_database_are_taken_from_the_arguments() {
        let args = parse(&["fifolio-server", "--port", "9123", "--database", "/x/y.db"]);
        assert_eq!(args.port, 9123);
        assert_eq!(args.database, PathBuf::from("/x/y.db"));
    }

    /// A port outside `u16` is refused by the parser, not wrapped [SRV-003].
    #[test]
    fn a_port_out_of_range_is_refused() {
        assert!(Args::try_parse_from(["fifolio-server", "--port", "70000"]).is_err());
    }

    /// `openapi` is a subcommand [SRV-005].
    #[test]
    fn openapi_is_a_subcommand() {
        assert_eq!(
            parse(&["fifolio-server", "openapi"]).command,
            Some(Command::Openapi)
        );
    }

    /// There is no way to ask for another interface: `--host` and `--bind` do not exist, and
    /// the one address is loopback [ARC-022].
    #[test]
    fn the_listening_address_is_loopback_only() {
        assert_eq!(
            listen_address(4321),
            "127.0.0.1:4321".parse::<SocketAddr>().expect("an address")
        );
        assert!(Args::try_parse_from(["fifolio-server", "--host", "0.0.0.0"]).is_err());
        assert!(Args::try_parse_from(["fifolio-server", "--bind", "0.0.0.0"]).is_err());
    }

    /// The document describes the route that serves it [SRV-006].
    #[test]
    fn the_spec_documents_its_own_route() {
        assert!(openapi().paths.paths.contains_key("/openapi.json"));
    }

    /// The problem document's schema is in the same spec as the routes that return it
    /// [ARC-020].
    #[test]
    fn the_spec_describes_the_problem_document() {
        let spec = serde_json::to_value(openapi()).expect("the spec serializes");
        let problem = &spec["components"]["schemas"]["Problem"];
        assert_eq!(
            problem["required"],
            serde_json::json!(["type", "title", "status"])
        );
        assert_eq!(problem["properties"]["status"]["type"], "integer");
    }
}
