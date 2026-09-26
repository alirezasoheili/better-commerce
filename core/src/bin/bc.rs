use better_commerce_core::reconcile::{ReconcileRequest, reconcile};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref() {
        Some(command) if command == "reconcile" => {}
        _ => return Err("usage: bc reconcile --manifest <path>".into()),
    }
    match (args.next().as_deref(), args.next()) {
        (Some(flag), Some(path)) if flag == "--manifest" => {
            if args.next().is_some() {
                return Err("usage: bc reconcile --manifest <path>".into());
            }
            let report = reconcile(ReconcileRequest {
                manifest_path: PathBuf::from(path),
            })
            .await?;
            println!(
                "Installation '{}' is ready at {} (database '{}').",
                report.installation_id, report.ready_url, report.database_name
            );
            Ok(())
        }
        _ => Err("usage: bc reconcile --manifest <path>".into()),
    }
}
