use std::collections::HashMap;
use std::env;
use std::process::Command;
use std::thread;

use camino::Utf8PathBuf;
use dropshot::{endpoint, UntypedBody};
use dropshot::{
    ApiDescription, ConfigDropshot, ConfigLogging, ConfigLoggingIfExists, ConfigLoggingLevel,
    HttpError, HttpResponseOk, HttpServerStarter, RequestContext,
};
use hmac::{Hmac, Mac};
use http::StatusCode;
use lettre::{Message, SendmailTransport, Transport};
use serde_json::Value;
use sha2::Sha256;

#[macro_use(slog_info)]
extern crate slog;

type HmacSha256 = Hmac<Sha256>;

struct ServerContext {
    ansible_path: String,
}

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

    openapi.description("API built with Dropshot for automated deployments.");

    // Panic if no .env file.
    dotenvy::dotenv().expect("Unable to load .env file.");

    // Get env var for path to ansible project, panic if it doesn't exist.
    let ansible_path =
        env::var("PATH_TO_ANSIBLE_PROJECT").expect("Unable to load ansible path from .env file.");

    // Start the server, passing ansible_path in context so it's available to endpoint.
    let context = ServerContext { ansible_path };
    let server = HttpServerStarter::new(
        &ConfigDropshot {
            bind_address: "127.0.0.1:7878".parse().unwrap(),
            request_body_max_bytes: 16384,
            tls: None,
        },
        api,
        context,
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
    rqctx: RequestContext<ServerContext>,
    body: UntypedBody,
) -> Result<HttpResponseOk<()>, HttpError> {
    let mut tag_map = HashMap::from([
        ("dvrpc/crash-api", "crash"),
        ("dvrpc/oced-econ-data", "econ_data"),
        ("dvrpc/low-stress-bike-routing", "low_stress_bike_routing"),
        ("dvrpc/lps-api", "lspv2"),
        ("dvrpc/sidewalk-priorities-api", "mcosp"),
        ("dvrpc/rtsp-api", "rtsp"),
        ("dvrpc/tp-updates", "tp_updates"),
        ("dvrpc/cjtf", "cjtf"),
    ]);

    // Get path from context.
    let context = rqctx.context();
    let ansible_path = serde_json::to_string_pretty(&context.ansible_path).unwrap();
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
            // Value.as_str() strips double quotes, but we also need it to be owned, so also
            // use to_string()
            Some(repo) => repo["full_name"].as_str().unwrap().to_string(),
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

    // Get corresponding tag to use in Ansible playbook.
    let tag = match tag_map.remove(name.as_str()) {
        None => {
            return Err(HttpError {
                status_code: StatusCode::BAD_REQUEST,
                error_code: None,
                external_message: format!("{} is not set up for automated deployment.", &name),
                internal_message: format!("{} is not set up for automated deployment.", &name),
            });
        }
        Some(v) => v,
    };

    // Github's webhooks have a 10-second timeout
    // (see <https://docs.github.com/en/webhooks/testing-and-troubleshooting-webhooks/troubleshooting-webhooks#timed-out>)
    // and since Ansible playbooks usually take much longer than this to run, we put it in a
    // thread so it can be done in the background and we can send a response to the webhook.
    // This means we must log the result separately from the response.

    let log = rqctx.log;

    thread::spawn(move || {
        let status = Command::new("ansible-playbook")
            .current_dir("/opt/cloud-ansible")
            .args([
                "playbook.yml",
                "-i",
                "inventories/from_controller.yaml",
                "-u",
                "controller",
                "--tags",
                tag,
            ])
            .status();

        let status = match status {
            Ok(v) => {
                if v.success() {
                    "success".to_string()
                } else {
                    "failure".to_string()
                }
            }
            Err(e) => e.to_string(),
        };
        slog_info!(log, "Ansible command completed"; "status" => status.clone());

        // Email the results to addresses in .env file. The message is built in separate chunks
        // b/c the number of addresses is unknown, otherwise it could all be chained at once.
        let receivers =
            env::var("EMAIL_RECEIVERS").expect("Unable to load email addreses from .env file");
        let receivers = receivers.split(',').collect::<Vec<_>>();

        let mut email = Message::builder().from(
            "Controller <root@controller.cloud.dvrpc.org>"
                .parse()
                .unwrap(),
        );

        for receiver in receivers.iter() {
            email = email.to(receiver.parse().unwrap());
        }

        let email = email
            .subject("Result from automated deployment API")
            .body(format!("Attempt to redeploy {name}: {status}."))
            .unwrap();

        // Use local sendmail program to send email.
        let sender = SendmailTransport::new();
        let _ = sender.send(&email);
    });

    Ok(HttpResponseOk(()))
}
