//! `mac_plan` — a tiny end-to-end demo of the read-only slice.
//!
//! Detects an installed Codex.app (if any), fetches the live prod appcast, and
//! prints the update plan (delta vs full, sizes, savings). Safe: never writes.
//!
//!   cargo run -p codex-mac-engine --bin mac_plan

use codex_mac_engine::{parse_appcast, plan_update, sys, UpdateStrategy};

const PROD_ARM64_APPCAST: &str = "https://codexapp.awai.cc/latest/appcast.xml";

fn mib(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn main() {
    // Optional CLI override: `mac_plan <build>` simulates an installed build.
    let arg_build = std::env::args().nth(1).and_then(|s| s.parse::<u64>().ok());
    let (source, build) = match arg_build {
        Some(b) => (format!("<override arg={b}>"), b),
        None => match sys::installed_codex_build() {
            Some(found) => found,
            None => {
                println!("(未检测到已装 Codex.app；用演示 build=3511 模拟“落后一版”)");
                ("<demo>".to_string(), 3511)
            }
        },
    };
    println!("已装 Codex: {source}  build={build}\n");

    let xml = match sys::fetch_text(PROD_ARM64_APPCAST) {
        Ok(xml) => xml,
        Err(e) => {
            eprintln!("拉取 appcast 失败: {e}");
            std::process::exit(1);
        }
    };

    let appcast = match parse_appcast(&xml) {
        Ok(appcast) => appcast,
        Err(e) => {
            eprintln!("解析 appcast 失败: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "appcast: {} 个条目, 最新 build {}",
        appcast.items.len(),
        appcast.latest().map(|i| i.build).unwrap_or(0)
    );

    match plan_update(&appcast, build) {
        Some(plan) if plan.up_to_date => {
            println!("✓ 已是最新 (build {})", plan.latest_build);
        }
        Some(plan) => {
            let kind = match plan.strategy {
                UpdateStrategy::Delta { from_build } => format!("DELTA (from {from_build})"),
                UpdateStrategy::Full => "FULL".to_string(),
            };
            println!("→ 计划: {kind}");
            println!(
                "  目标: {} (build {})",
                plan.latest_short_version, plan.latest_build
            );
            println!(
                "  下载: {:.1} MB   全量: {:.1} MB   节省: {:.1}%",
                mib(plan.download_size),
                mib(plan.full_size),
                plan.savings_pct
            );
            println!("  URL : {}", plan.download_url);
            println!(
                "  edSig: {}",
                plan.ed_signature.as_deref().unwrap_or("(none)")
            );
        }
        None => println!("无法生成计划（appcast 为空？）"),
    }
}
