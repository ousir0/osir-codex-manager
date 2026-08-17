//! `mac_fetch [build]` — end-to-end proof of the *safe* download+verify slice.
//!
//! Plans an update from the given installed build, downloads the delta/full to
//! a staging dir, checks the size, and verifies the Ed25519 signature against
//! the pinned Sparkle key. Performs NO install/replace.
//!
//!   cargo run -p codex-mac-engine --bin mac_fetch -- 3511

use std::path::PathBuf;

use codex_mac_engine::{download, parse_appcast, plan_update, sys, verify, UpdateStrategy};

const PROD_ARM64_APPCAST: &str = "https://app.osirclaw.com/latest/appcast.xml";
const STAGING_DIR: &str = "/tmp/codex-mac-staging";

fn mib(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn main() {
    let build: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .or_else(|| sys::installed_codex_build().map(|(_, b)| b))
        .unwrap_or(3511);
    println!("基准已装 build = {build}\n");

    let xml = sys::fetch_text(PROD_ARM64_APPCAST).expect("拉取 appcast 失败");
    let appcast = parse_appcast(&xml).expect("解析 appcast 失败");
    let plan = plan_update(&appcast, build).expect("生成计划失败");

    if plan.up_to_date {
        println!("✓ 已是最新 (build {})，无需下载。", plan.latest_build);
        return;
    }

    let kind = match &plan.strategy {
        UpdateStrategy::Delta { from_build } => format!("DELTA (from {from_build})"),
        UpdateStrategy::Full => "FULL".to_string(),
    };
    let sig = plan
        .ed_signature
        .clone()
        .expect("appcast enclosure 缺少 edSignature");
    println!(
        "计划: {kind} → {} (build {})\n下载 {:.1} MB / 全量 {:.1} MB (省 {:.1}%)\nURL: {}\n",
        plan.latest_short_version,
        plan.latest_build,
        mib(plan.download_size),
        mib(plan.full_size),
        plan.savings_pct,
        plan.download_url
    );

    let file_name = plan
        .download_url
        .rsplit('/')
        .next()
        .unwrap_or("payload.bin");
    let dest = PathBuf::from(STAGING_DIR).join(file_name);

    // Reuse an already-staged artifact of the right size (idempotent re-runs).
    let already = std::fs::metadata(&dest)
        .map(|m| m.len() == plan.download_size)
        .unwrap_or(false);
    if already {
        println!("staging 已存在且大小匹配，跳过下载: {}", dest.display());
    } else {
        println!("下载到 staging…");
        let n = download::download_to(&plan.download_url, &dest).expect("下载失败");
        println!("下载完成: {:.1} MB", mib(n));
    }

    // 1) size gate
    let len = std::fs::metadata(&dest).expect("stat staging").len();
    assert_eq!(len, plan.download_size, "大小不匹配，拒绝");
    println!("✓ 大小校验通过: {len} bytes");

    // 2) EdDSA gate (pinned key)
    let bytes = download::read_file(&dest).expect("读取 staging 失败");
    match verify::verify_sparkle(&bytes, &sig) {
        Ok(()) => println!(
            "✅ EdDSA 验签通过（钉死公钥 {})",
            verify::SPARKLE_ED_PUBKEY_B64
        ),
        Err(e) => {
            eprintln!("❌ EdDSA 验签失败: {e}");
            std::process::exit(1);
        }
    }

    println!("\n（已安全落到 staging 并通过 大小+EdDSA 双闸，未触碰任何安装根。下一步才是 BinaryDelta 应用 → codesign 复验 → 原子替换。）");
}
