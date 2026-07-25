use aria_focus_desktop_lib::music_batch::{
    CloudGenerationRequest, CloudGenerationService, CloudKeyStatus,
};
use serde_json::json;
use std::env;
use std::path::PathBuf;
use std::time::Duration;

const AUDIO_MODEL: &str = "google/lyria-3-pro-preview";
const TEXT_MODEL: &str = "google/gemini-2.5-flash";
const IMAGE_MODEL: &str = "google/gemini-2.5-flash-image";

fn usage() -> ! {
    eprintln!(
        "Usage: cloud-generation-cli --confirm-paid [--count N] [--duration SECONDS] \\
         [--activities activity1,activity2] [--budget-usd USD] [--note TEXT] \\
         [--no-prompt-refinement] [--no-covers] [--wait] [--activate] [--quiet] \\
         [--resume-batch BATCH_ID]"
    );
    std::process::exit(2);
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn parse_u16(args: &[String], name: &str, default: u16) -> u16 {
    value(args, name)
        .as_deref()
        .map(|raw| raw.parse::<u16>().unwrap_or_else(|_| usage()))
        .unwrap_or(default)
}

fn parse_budget(args: &[String]) -> u64 {
    let value = value(args, "--budget-usd").unwrap_or_else(|| "0.10".into());
    let amount = value.parse::<f64>().unwrap_or_else(|_| usage());
    if !amount.is_finite() || amount < 0.0 {
        usage();
    }
    (amount * 1_000_000.0).round() as u64
}

fn app_data_root() -> PathBuf {
    if let Some(appdata) = env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("com.ariazanganeh.ariafocus");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aria-focus")
}

fn key_status(service: &CloudGenerationService) -> CloudKeyStatus {
    service.key_status().unwrap_or_else(|error| {
        eprintln!("Cannot read OpenRouter key status: {error}");
        std::process::exit(2)
    })
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") || args.is_empty() {
        usage();
    }
    let root = app_data_root();
    let quiet = args.iter().any(|arg| arg == "--quiet");
    let service =
        CloudGenerationService::new(root.join("preferences.sqlite3"), root.join("content"));
    let status = key_status(&service);
    if !status.mock && !args.iter().any(|arg| arg == "--confirm-paid") {
        eprintln!("This command can spend OpenRouter credits. Add --confirm-paid after reviewing the estimate.");
        std::process::exit(2);
    }

    let count = parse_u16(&args, "--count", 1);
    let duration = parse_u16(&args, "--duration", 180);
    let activities = value(&args, "--activities")
        .unwrap_or_else(|| "motivation".into())
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let refine_prompts = !args.iter().any(|arg| arg == "--no-prompt-refinement");
    let generate_covers = !args.iter().any(|arg| arg == "--no-covers");
    let request = CloudGenerationRequest {
        target_count: count,
        activities,
        audio_model: value(&args, "--audio-model").unwrap_or_else(|| AUDIO_MODEL.into()),
        text_model: refine_prompts
            .then(|| value(&args, "--text-model").unwrap_or_else(|| TEXT_MODEL.into())),
        image_model: generate_covers
            .then(|| value(&args, "--image-model").unwrap_or_else(|| IMAGE_MODEL.into())),
        refine_prompts,
        generate_covers,
        duration_seconds: duration,
        budget_microdollars: parse_budget(&args),
        note: value(&args, "--note"),
    };
    let estimate = service.estimate(&request).unwrap_or_else(|error| {
        eprintln!("Estimate failed: {error}");
        std::process::exit(2)
    });
    if !quiet {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"estimate": estimate})).unwrap()
        );
    } else {
        println!(
            "estimate_usd={:.4} target_count={}",
            estimate.total_microdollars as f64 / 1_000_000.0,
            estimate.target_count
        );
    }
    let mut batch = if let Some(batch_id) = value(&args, "--resume-batch") {
        service
            .resume_batch(&batch_id, request)
            .unwrap_or_else(|error| {
                eprintln!("Batch resume failed: {error}");
                std::process::exit(2)
            })
    } else {
        service.create_batch(request).unwrap_or_else(|error| {
            eprintln!("Batch creation failed: {error}");
            std::process::exit(2)
        })
    };
    if !quiet {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"batch": batch})).unwrap()
        );
    } else {
        println!("batch_id={} state={}", batch.batch_id, batch.state);
    }
    if !args.iter().any(|arg| arg == "--wait") {
        return;
    }
    loop {
        std::thread::sleep(Duration::from_secs(2));
        batch = service
            .get_batch(&batch.batch_id)
            .unwrap_or_else(|error| {
                eprintln!("Batch status failed: {error}");
                std::process::exit(2)
            })
            .unwrap_or_else(|| {
                eprintln!("Batch disappeared.");
                std::process::exit(2)
            });
        if !quiet {
            println!(
                "{}",
                serde_json::to_string(&json!({"batch": batch})).unwrap()
            );
        }
        if matches!(batch.state.as_str(), "validated" | "failed" | "cancelled") {
            break;
        }
    }
    let items = service.get_items(&batch.batch_id).unwrap_or_else(|error| {
        eprintln!("Item inspection failed: {error}");
        std::process::exit(2)
    });
    if quiet {
        println!(
            "terminal_state={} completed={} failed={}",
            batch.state, batch.completed_count, batch.failed_count
        );
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"items": items})).unwrap()
        );
    }
    if batch.state == "validated" && args.iter().any(|arg| arg == "--activate") {
        let active = service
            .activate_batch(&batch.batch_id)
            .unwrap_or_else(|error| {
                eprintln!("Activation failed: {error}");
                std::process::exit(2)
            });
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"activated": active})).unwrap()
        );
    }
}
