use std::env;
use std::process::Command;

use camino::Utf8PathBuf;
use dropshot::{endpoint, UntypedBody};
use dropshot::{
    ApiDescription, ConfigDropshot, ConfigLogging, ConfigLoggingIfExists, ConfigLoggingLevel,
    HttpError, HttpResponseOk, HttpServerStarter, RequestContext,
};
use hmac::{Hmac, Mac};
use http::StatusCode;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

// #[derive(Serialize, JsonSchema)]
// struct CommandResult {
//     success: bool,
//     exit_status_code: Option<i32>,
// }

#[tokio::main]
async fn main() -> Result<(), String> {
    // Set up a logger.
    let log = ConfigLogging::File {
        level: ConfigLoggingLevel::Info,
        path: Utf8PathBuf::from("api.log"),
        if_exists: ConfigLoggingIfExists::Append,
    }
    .to_logger("automated-deployments")
    .map_err(|e| e.to_string())?;

    // Describe the API.
    let mut api = ApiDescription::new();

    /*
     * Register our endpoint and its handler function.  The "endpoint" macro
     * specifies the HTTP method and URI path that identify the endpoint,
     * allowing this metadata to live right alongside the handler function.
     */
    api.register(post_webhook).unwrap();

    // Create an OpenAPI definition, adding title and version.
    let mut openapi = api.openapi("DVRPC Automated Deployments API", "0.1.0");

    // Add description.
    openapi.description("API built with Dropshot for automated deployments.");

    // Panic if no .env file.
    dotenvy::dotenv().expect("Unable to load .env file.");

    // Start the server.
    let server = HttpServerStarter::new(
        &ConfigDropshot {
            bind_address: "127.0.0.1:7878".parse().unwrap(),
            request_body_max_bytes: 16384,
            tls: None,
        },
        api,
        (),
        &log,
    )
    .map_err(|error| format!("failed to start server: {}", error))?
    .start();

    server.await
}

/// Handle webhooks for potential automated deployment
#[endpoint {
    method = POST,
    path = "/api/ad"
}]
async fn post_webhook(
    rqctx: RequestContext<()>,
    body: UntypedBody,
) -> Result<HttpResponseOk<()>, HttpError> {
    // Get required header
    let headers = rqctx.request.headers();
    if !headers.contains_key("x-hub-signature-256") {
        return Err(HttpError {
            status_code: StatusCode::FORBIDDEN,
            error_code: None,
            external_message: "Required header not provided.".to_string(),
            internal_message: "Required header not provided.".to_string(),
        });
    }
    let received_hash = match headers["x-hub-signature-256"].to_str() {
        Ok(v) => Ok(v.trim_start_matches("sha256=").trim_end()),
        Err(e) => Err(HttpError {
            status_code: StatusCode::FORBIDDEN,
            error_code: None,
            external_message: "Unable to decode signature header.".to_string(),
            internal_message: e.to_string(),
        }),
    }?;

    // Get the secret from .env.
    let secret = match dotenvy::dotenv() {
        Ok(_) => match env::var("GITHUB_TOKEN") {
            Ok(v) => Ok(v),
            Err(e) => Err(HttpError {
                status_code: StatusCode::INTERNAL_SERVER_ERROR,
                error_code: None,
                external_message: "Unable to verify token.".to_string(),
                internal_message: e.to_string(),
            }),
        },
        Err(e) => Err(HttpError {
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
            error_code: None,
            external_message: "Unable to verify token.".to_string(),
            internal_message: e.to_string(),
        }),
    }?;

    // Compute the hash from our secret and the received body, compare with signature in header.
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(v) => v,
        Err(e) => {
            return Err(HttpError {
                status_code: StatusCode::INTERNAL_SERVER_ERROR,
                error_code: None,
                external_message: "Unable to verify token.".to_string(),
                internal_message: e.to_string(),
            })
        }
    };
    mac.update(body.as_bytes());
    let computed_hash = format!("{:x}", mac.finalize().into_bytes());

    if computed_hash != received_hash {
        return Err(HttpError {
            status_code: StatusCode::FORBIDDEN,
            error_code: None,
            external_message: "Invalid token.".to_string(),
            internal_message: "Mismatched hashes".to_string(),
        });
    }

    // Determine what app/API this is for.
    let name = match serde_json::from_slice::<Value>(body.as_bytes()) {
        Ok(v) => match v.get("repository") {
            Some(repo) => repo["full_name"].to_string(),
            None => {
                return Err(HttpError {
                    status_code: StatusCode::BAD_REQUEST,
                    error_code: None,
                    external_message: "Unable to get repository field from webhook.".to_string(),
                    internal_message: "Unable to get repository field from webhook.".to_string(),
                });
            }
        },
        Err(_) => {
            return Err(HttpError {
                status_code: StatusCode::BAD_REQUEST,
                error_code: None,
                external_message: "Unable to get body of request as json.".to_string(),
                internal_message: "Unable to get body of request as json.".to_string(),
            });
        }
    };

    dbg!(&name);

    // Run the appropriate part of Ansible playbook
    // match Command::new("ansible-playbook")
    //     .current_dir("/srv/cloud-ansible")
    //     .args([
    //         "controler_playbook.yaml",
    //         "-i",
    //         "inventories/control.yaml",
    //         "--tags", // another parameter to follow with appropriate tag, based on repo
    //     ])
    //     .output()
    // {
    //     Ok(v) if !v.status.success() => Err(HttpError {
    //         status_code: StatusCode::INTERNAL_SERVER_ERROR,
    //         error_code: None,
    //         external_message: "Program did not complete successfully.".to_string(),
    //         internal_message: String::from_utf8_lossy(&v.stderr).into_owned(),
    //     }),
    //     Ok(_) => Ok(HttpResponseOk(())),
    //     Err(e) => Err(HttpError {
    //         status_code: StatusCode::INTERNAL_SERVER_ERROR,
    //         error_code: None,
    //         external_message: "Internal server error".to_string(),
    //         internal_message: format!("Error trying to run the program: {}", e),
    //     }),
    // }

    Ok(HttpResponseOk(()))
}
