use crate::ai::compact_error_body;
use crate::domain::{
    AppState, UpdateCheckResult, UpdateDownloadResult, UpdateInstallResult, UpdateProgress,
};
use crate::{
    sanitize_file_name, to_string, GITHUB_LATEST_RELEASE_API_URL, GITHUB_LATEST_RELEASE_URL,
    GITHUB_REPOSITORY_URL,
};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tauri::{Emitter, State};
use tokio::time::timeout;
use zip::ZipArchive;

const UPDATE_USER_AGENT: &str = "YuriRewrite";
const UPDATE_SOURCE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATE_SOURCE_TOTAL_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_PORTABLE_ENTRY_SIZE: u64 = 220 * 1024 * 1024;
const MAX_PORTABLE_TOTAL_SIZE: u64 = 250 * 1024 * 1024;
const UPDATE_RESULT_FILE: &str = "last-result.json";
const PORTABLE_EXE_NAME: &str = "Yuri Rewrite.exe";
const PORTABLE_README_NAME: &str = "README.md";

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
    size: u64,
}

#[derive(Debug, Clone)]
struct DownloadSource {
    label: &'static str,
    url: String,
}

#[tauri::command]
pub(crate) fn open_github_url() -> Result<(), String> {
    open_url_in_default_browser(GITHUB_REPOSITORY_URL)
}

#[tauri::command]
pub(crate) fn open_github_release_url() -> Result<(), String> {
    open_url_in_default_browser(GITHUB_LATEST_RELEASE_URL)
}

#[tauri::command]
pub(crate) async fn check_for_updates(
    state: State<'_, AppState>,
) -> Result<UpdateCheckResult, String> {
    fetch_latest_update(&state.client).await
}

#[tauri::command]
pub(crate) async fn download_latest_update(
    state: State<'_, AppState>,
) -> Result<UpdateDownloadResult, String> {
    ensure_update_can_start(&state)?;
    let update = fetch_latest_update(&state.client).await?;
    if update.is_latest {
        return Err(format!("当前已是最新版：{}", update.current_version));
    }

    let update_root = state
        .data_dir
        .join("updates")
        .join(format!("update-{}", sanitize_file_name(&update.latest_version)));
    fs::create_dir_all(&update_root).map_err(to_string)?;
    let zip_path = update_root.join(sanitize_file_name(&update.asset_name));
    let sources = update_download_sources(&update.asset_download_url);
    download_from_sources(
        &state,
        &sources,
        &zip_path,
        update.asset_size,
        &update.release_url,
    )
    .await?;

    emit_update_progress(
        &state,
        UpdateProgress {
            stage: "validating".to_string(),
            source: None,
            downloaded_bytes: update.asset_size.unwrap_or(0),
            total_bytes: update.asset_size,
            message: "下载完成，正在校验更新包…".to_string(),
        },
    );
    validate_portable_zip(&zip_path)?;

    let digest = update.asset_digest.as_deref().and_then(normalize_sha256_digest);
    let auto_install_reason = update
        .auto_install_reason
        .clone()
        .or_else(|| digest.is_none().then(|| "GitHub 未提供该资产的 SHA-256 摘要，不能安全自动安装。".to_string()));

    if !update.auto_install_supported || digest.is_none() {
        let manual_path = copy_update_for_manual_install(&zip_path, &update.asset_name, &state)?;
        return Ok(UpdateDownloadResult {
            path: manual_path.to_string_lossy().to_string(),
            version: update.latest_version,
            install_started: false,
            manual_install_required: true,
            message: auto_install_reason.unwrap_or_else(|| "当前运行环境不支持自动安装。".to_string()),
        });
    }

    verify_sha256(&zip_path, digest.as_deref().expect("digest checked above"))?;
    let current_exe = env::current_exe().map_err(to_string)?;
    let install = portable_install_paths(&current_exe, &update.latest_version)?;
    verify_parent_is_writable(&install.parent_dir)?;

    emit_update_progress(
        &state,
        UpdateProgress {
            stage: "preparing".to_string(),
            source: None,
            downloaded_bytes: update.asset_size.unwrap_or(0),
            total_bytes: update.asset_size,
            message: "校验通过，正在准备关闭软件并安装更新…".to_string(),
        },
    );

    let script_path = update_root.join("apply-update.ps1");
    let log_path = update_root.join("update.log");
    let result_path = state.data_dir.join("updates").join(UPDATE_RESULT_FILE);
    let script = build_updater_script(UpdaterScriptOptions {
        current_pid: std::process::id(),
        current_dir: &install.current_dir,
        current_exe: &current_exe,
        target_dir: &install.target_dir,
        zip_path: &zip_path,
        log_path: &log_path,
        result_path: &result_path,
        version: &update.latest_version,
    });
    write_utf8_bom(&script_path, &script)?;
    spawn_updater(&script_path)?;

    emit_update_progress(
        &state,
        UpdateProgress {
            stage: "restarting".to_string(),
            source: None,
            downloaded_bytes: update.asset_size.unwrap_or(0),
            total_bytes: update.asset_size,
            message: "即将关闭软件。更新完成后会自动重新打开…".to_string(),
        },
    );

    let app = state.app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(800)).await;
        app.exit(0);
    });

    Ok(UpdateDownloadResult {
        path: zip_path.to_string_lossy().to_string(),
        version: update.latest_version,
        install_started: true,
        manual_install_required: false,
        message: "更新器已启动，软件即将重启。".to_string(),
    })
}

#[tauri::command]
pub(crate) fn take_update_install_result(
    state: State<'_, AppState>,
) -> Result<Option<UpdateInstallResult>, String> {
    let path = state.data_dir.join("updates").join(UPDATE_RESULT_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(to_string)?;
    let result = serde_json::from_str::<UpdateInstallResult>(&content).map_err(to_string)?;
    fs::remove_file(path).map_err(to_string)?;
    Ok(Some(result))
}

pub(crate) async fn fetch_latest_update(client: &Client) -> Result<UpdateCheckResult, String> {
    match fetch_latest_release_api(client).await {
        Ok(result) => Ok(result),
        Err(api_error) => fetch_latest_release_redirect(client)
            .await
            .map_err(|redirect_error| {
                format!(
                    "检查更新失败。GitHub API：{}；发布页：{}",
                    api_error, redirect_error
                )
            }),
    }
}

async fn fetch_latest_release_api(client: &Client) -> Result<UpdateCheckResult, String> {
    let response = client
        .get(GITHUB_LATEST_RELEASE_API_URL)
        .header("User-Agent", UPDATE_USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(to_string)?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.map_err(to_string)?;
        return Err(format!(
            "HTTP {}: {}",
            status,
            compact_error_body(&body)
        ));
    }
    let release = response.json::<GithubRelease>().await.map_err(to_string)?;
    let latest_version = normalize_release_version(&release.tag_name);
    let expected_name = portable_zip_name(&latest_version);
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == expected_name)
        .ok_or_else(|| format!("最新 Release 中未找到 {}", expected_name))?;
    Ok(build_update_check_result(
        release.tag_name,
        release.html_url,
        asset.name,
        asset.browser_download_url,
        asset.digest,
        Some(asset.size),
    ))
}

async fn fetch_latest_release_redirect(client: &Client) -> Result<UpdateCheckResult, String> {
    let response = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .header("User-Agent", UPDATE_USER_AGENT)
        .send()
        .await
        .map_err(to_string)?;
    let status = response.status();
    let final_url = response.url().to_string();
    if !status.is_success() {
        let body = response.text().await.map_err(to_string)?;
        return Err(format!(
            "HTTP {}: {}",
            status,
            compact_error_body(&body)
        ));
    }

    let latest_tag = release_tag_from_url(&final_url)
        .ok_or_else(|| format!("无法从 GitHub 最新发布地址解析版本：{}", final_url))?;
    let latest_version = normalize_release_version(&latest_tag);
    let release_url = format!("{}/releases/tag/{}", GITHUB_REPOSITORY_URL, latest_tag);
    let asset_name = portable_zip_name(&latest_version);
    let asset_download_url = format!(
        "{}/releases/download/{}/{}",
        GITHUB_REPOSITORY_URL, latest_tag, asset_name
    );
    Ok(build_update_check_result(
        latest_tag,
        release_url,
        asset_name,
        asset_download_url,
        None,
        None,
    ))
}

fn build_update_check_result(
    latest_tag: String,
    release_url: String,
    asset_name: String,
    asset_download_url: String,
    asset_digest: Option<String>,
    asset_size: Option<u64>,
) -> UpdateCheckResult {
    let latest_version = normalize_release_version(&latest_tag);
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let (auto_install_supported, auto_install_reason) = match env::current_exe() {
        Ok(path) => match portable_install_paths(&path, &latest_version) {
            Ok(_) => (true, None),
            Err(reason) => (false, Some(reason)),
        },
        Err(error) => (false, Some(format!("无法确定当前程序路径：{}", error))),
    };
    UpdateCheckResult {
        current_version: current_version.clone(),
        latest_version: latest_version.clone(),
        latest_tag,
        is_latest: !is_newer_version(&latest_version, &current_version),
        release_url,
        asset_name,
        asset_download_url,
        asset_digest,
        asset_size,
        auto_install_supported,
        auto_install_reason,
    }
}

async fn download_from_sources(
    state: &State<'_, AppState>,
    sources: &[DownloadSource],
    output_path: &Path,
    expected_size: Option<u64>,
    release_url: &str,
) -> Result<(), String> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(to_string)?;
    let mut failures = Vec::new();

    for (index, source) in sources.iter().enumerate() {
        if index > 0 {
            emit_update_progress(
                state,
                UpdateProgress {
                    stage: "switching".to_string(),
                    source: Some(source.label.to_string()),
                    downloaded_bytes: 0,
                    total_bytes: expected_size,
                    message: format!("GitHub 下载未完成，正在切换到{}…", source.label),
                },
            );
        }
        match download_one_source(state, &client, source, output_path, expected_size).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                failures.push(format!("{}：{}", source.label, error));
                let _ = fs::remove_file(output_path);
            }
        }
    }

    Err(format!(
        "自动下载失败，请手动访问 GitHub 发布页下载 portable ZIP：{}。{}",
        release_url,
        failures.join("；")
    ))
}

async fn download_one_source(
    state: &State<'_, AppState>,
    client: &Client,
    source: &DownloadSource,
    output_path: &Path,
    expected_size: Option<u64>,
) -> Result<(), String> {
    emit_update_progress(
        state,
        UpdateProgress {
            stage: "downloading".to_string(),
            source: Some(source.label.to_string()),
            downloaded_bytes: 0,
            total_bytes: expected_size,
            message: format!("正在从{}下载最新版…", source.label),
        },
    );
    let started = Instant::now();
    let response = timeout(
        UPDATE_SOURCE_IDLE_TIMEOUT,
        client
            .get(&source.url)
            .header("User-Agent", UPDATE_USER_AGENT)
            .send(),
    )
    .await
    .map_err(|_| "30 秒内未收到响应".to_string())?
    .map_err(to_string)?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.map_err(to_string)?;
        return Err(format!(
            "HTTP {}: {}",
            status,
            compact_error_body(&body)
        ));
    }

    let response_total = response.content_length().or(expected_size);
    let mut stream = response.bytes_stream();
    let mut file = File::create(output_path).map_err(to_string)?;
    let mut downloaded = 0_u64;
    let mut last_emit = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);

    loop {
        if started.elapsed() >= UPDATE_SOURCE_TOTAL_TIMEOUT {
            return Err("单个下载源超过 5 分钟仍未完成".to_string());
        }
        let remaining = UPDATE_SOURCE_TOTAL_TIMEOUT.saturating_sub(started.elapsed());
        let wait = UPDATE_SOURCE_IDLE_TIMEOUT.min(remaining);
        let next = timeout(wait, stream.next())
            .await
            .map_err(|_| "连续 30 秒没有下载进展".to_string())?;
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk.map_err(to_string)?;
        file.write_all(&chunk).map_err(to_string)?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if last_emit.elapsed() >= Duration::from_millis(400) {
            emit_update_progress(
                state,
                UpdateProgress {
                    stage: "downloading".to_string(),
                    source: Some(source.label.to_string()),
                    downloaded_bytes: downloaded,
                    total_bytes: response_total,
                    message: format!(
                        "正在从{}下载最新版：{}",
                        source.label,
                        format_download_size(downloaded, response_total)
                    ),
                },
            );
            last_emit = Instant::now();
        }
    }
    file.flush().map_err(to_string)?;

    if let Some(size) = expected_size {
        if downloaded != size {
            return Err(format!(
                "下载文件大小不匹配，预期 {} 字节，实际 {} 字节",
                size, downloaded
            ));
        }
    }
    if downloaded == 0 {
        return Err("下载结果为空".to_string());
    }
    Ok(())
}

fn emit_update_progress(state: &State<'_, AppState>, progress: UpdateProgress) {
    let _ = state.app.emit("update-progress", progress);
}

fn update_download_sources(original_url: &str) -> Vec<DownloadSource> {
    [
        ("GitHub", original_url.to_string()),
        ("国内镜像 1", format!("https://gh-proxy.com/{}", original_url)),
        ("国内镜像 2", format!("https://ghproxy.net/{}", original_url)),
        ("国内镜像 3", format!("https://ghfast.top/{}", original_url)),
    ]
    .into_iter()
    .map(|(label, url)| DownloadSource { label, url })
    .collect()
}

fn format_download_size(downloaded: u64, total: Option<u64>) -> String {
    let downloaded_mb = downloaded as f64 / 1024.0 / 1024.0;
    match total {
        Some(total) if total > 0 => format!(
            "{:.1} / {:.1} MB",
            downloaded_mb,
            total as f64 / 1024.0 / 1024.0
        ),
        _ => format!("{:.1} MB", downloaded_mb),
    }
}

fn validate_portable_zip(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(to_string)?;
    let mut archive = ZipArchive::new(file).map_err(to_string)?;
    let expected = BTreeSet::from([
        PORTABLE_EXE_NAME.to_string(),
        PORTABLE_README_NAME.to_string(),
    ]);
    let mut actual = BTreeSet::new();
    let mut total_size = 0_u64;

    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(to_string)?;
        let name = entry.name().replace('\\', "/");
        if entry.is_dir() {
            return Err(format!("更新包包含不允许的目录：{}", name));
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| format!("更新包包含不安全路径：{}", name))?;
        if enclosed.components().count() != 1
            || enclosed
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!("更新包包含嵌套或绝对路径：{}", name));
        }
        if entry.size() > MAX_PORTABLE_ENTRY_SIZE {
            return Err(format!("更新包文件异常过大：{}", name));
        }
        total_size = total_size.saturating_add(entry.size());
        if total_size > MAX_PORTABLE_TOTAL_SIZE {
            return Err("更新包解压后体积异常过大".to_string());
        }
        actual.insert(name);
    }

    if actual != expected {
        return Err(format!(
            "更新包内容无效，应仅包含 {} 和 {}，实际为：{}",
            PORTABLE_EXE_NAME,
            PORTABLE_README_NAME,
            actual.into_iter().collect::<Vec<_>>().join("、")
        ));
    }
    Ok(())
}

fn normalize_sha256_digest(value: &str) -> Option<String> {
    let digest = value
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(value.trim())
        .to_ascii_lowercase();
    (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(digest)
}

fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = File::open(path).map_err(to_string)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(to_string)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "更新包 SHA-256 校验失败，预期 {}，实际 {}",
            expected, actual
        ))
    }
}

struct PortableInstallPaths {
    current_dir: PathBuf,
    target_dir: PathBuf,
    parent_dir: PathBuf,
}

fn portable_install_paths(
    current_exe: &Path,
    latest_version: &str,
) -> Result<PortableInstallPaths, String> {
    let latest_version = safe_release_version(latest_version)?;
    let exe_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "当前程序文件名无效，不能自动安装。".to_string())?;
    if !exe_name.eq_ignore_ascii_case(PORTABLE_EXE_NAME) {
        return Err("当前不是从标准 portable 目录运行，只能下载后手动安装。".to_string());
    }
    let current_dir = current_exe
        .parent()
        .ok_or_else(|| "当前程序目录无效，不能自动安装。".to_string())?
        .to_path_buf();
    let dir_name = current_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "当前 portable 目录名无效，不能自动安装。".to_string())?;
    if !dir_name.starts_with("YuriRewrite-v") || !dir_name.ends_with("-windows-x64") {
        return Err("当前目录不是标准 YuriRewrite-v版本号-windows-x64 portable 目录。".to_string());
    }
    let parent_dir = current_dir
        .parent()
        .ok_or_else(|| "当前 portable 目录没有可用的上级目录。".to_string())?
        .to_path_buf();
    let target_dir = parent_dir.join(format!(
        "YuriRewrite-v{}-windows-x64",
        latest_version
    ));
    if target_dir == current_dir {
        return Err("目标版本目录与当前目录相同。".to_string());
    }
    Ok(PortableInstallPaths {
        current_dir,
        target_dir,
        parent_dir,
    })
}

fn safe_release_version(version: &str) -> Result<String, String> {
    let version = normalize_release_version(version);
    if version.is_empty()
        || version.contains("..")
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
    {
        return Err("Release 版本号包含不安全字符。".to_string());
    }
    Ok(version)
}

fn verify_parent_is_writable(parent: &Path) -> Result<(), String> {
    let probe = parent.join(format!(".yuri-update-write-test-{}", std::process::id()));
    File::create(&probe)
        .and_then(|mut file| file.write_all(b"ok"))
        .map_err(|error| format!("portable 上级目录不可写，不能自动安装：{}", error))?;
    fs::remove_file(probe)
        .map_err(|error| format!("无法清理更新写入测试文件：{}", error))
}

fn copy_update_for_manual_install(
    source: &Path,
    asset_name: &str,
    state: &State<'_, AppState>,
) -> Result<PathBuf, String> {
    let output_dir = default_download_dir().unwrap_or_else(|| state.data_dir.join("updates"));
    fs::create_dir_all(&output_dir).map_err(to_string)?;
    let destination = output_dir.join(sanitize_file_name(asset_name));
    fs::copy(source, &destination).map_err(to_string)?;
    Ok(destination)
}

struct UpdaterScriptOptions<'a> {
    current_pid: u32,
    current_dir: &'a Path,
    current_exe: &'a Path,
    target_dir: &'a Path,
    zip_path: &'a Path,
    log_path: &'a Path,
    result_path: &'a Path,
    version: &'a str,
}

fn build_updater_script(options: UpdaterScriptOptions<'_>) -> String {
    let parent_dir = options
        .current_dir
        .parent()
        .unwrap_or(options.current_dir);
    let staging_dir = parent_dir.join(format!(
        ".yuri-update-staging-{}",
        sanitize_file_name(options.version)
    ));
    format!(
        r#"$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$CurrentPid = {current_pid}
$CurrentDir = '{current_dir}'
$CurrentExe = '{current_exe}'
$TargetDir = '{target_dir}'
$StagingDir = '{staging_dir}'
$ZipPath = '{zip_path}'
$LogPath = '{log_path}'
$ResultPath = '{result_path}'
$Version = '{version}'
$Timestamp = Get-Date -Format "yyyyMMddHHmmss"
$BackupDir = "$CurrentDir.old-$Timestamp"
$MovedCurrent = $false

function Write-Log([string]$Message) {{
  $Line = "$(Get-Date -Format o) $Message"
  Add-Content -LiteralPath $LogPath -Value $Line -Encoding UTF8
}}

function Write-Result([string]$Status, [string]$Message) {{
  $Payload = @{{
    status = $Status
    version = $Version
    message = $Message
    log_path = $LogPath
  }} | ConvertTo-Json -Compress
  [System.IO.File]::WriteAllText($ResultPath, $Payload, (New-Object System.Text.UTF8Encoding($false)))
}}

try {{
  Write-Log "等待主程序退出。"
  Wait-Process -Id $CurrentPid -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 500
  if (Test-Path -LiteralPath $TargetDir) {{
    throw "目标版本目录已存在：$TargetDir"
  }}
  if (Test-Path -LiteralPath $StagingDir) {{
    Remove-Item -LiteralPath $StagingDir -Recurse -Force
  }}
  New-Item -ItemType Directory -Path $StagingDir | Out-Null
  Expand-Archive -LiteralPath $ZipPath -DestinationPath $StagingDir -Force
  $StagedExe = Join-Path $StagingDir '{portable_exe}'
  if (!(Test-Path -LiteralPath $StagedExe)) {{
    throw "解压后未找到 {portable_exe}"
  }}

  $Moved = $false
  for ($Attempt = 0; $Attempt -lt 30 -and !$Moved; $Attempt++) {{
    try {{
      Move-Item -LiteralPath $CurrentDir -Destination $BackupDir
      $Moved = $true
      $MovedCurrent = $true
    }} catch {{
      if ($Attempt -eq 29) {{ throw }}
      Start-Sleep -Milliseconds 500
    }}
  }}
  Move-Item -LiteralPath $StagingDir -Destination $TargetDir
  $NewExe = Join-Path $TargetDir '{portable_exe}'
  Start-Process -FilePath $NewExe -WorkingDirectory $TargetDir | Out-Null
  Write-Result "success" "已更新到 v$Version。"
  Write-Log "新版程序已启动。"

  try {{
    Start-Sleep -Seconds 2
    if (Test-Path -LiteralPath $BackupDir) {{
      Remove-Item -LiteralPath $BackupDir -Recurse -Force
    }}
    if (Test-Path -LiteralPath $ZipPath) {{
      Remove-Item -LiteralPath $ZipPath -Force
    }}
  }} catch {{
    Write-Log "更新成功，但清理旧文件失败：$($_.Exception.Message)"
  }}
}} catch {{
  $Failure = $_.Exception.Message
  Write-Log "更新失败：$Failure"
  try {{
    if (Test-Path -LiteralPath $StagingDir) {{
      Remove-Item -LiteralPath $StagingDir -Recurse -Force
    }}
    if (Test-Path -LiteralPath $TargetDir) {{
      Remove-Item -LiteralPath $TargetDir -Recurse -Force
    }}
    if ($MovedCurrent -and (Test-Path -LiteralPath $BackupDir) -and !(Test-Path -LiteralPath $CurrentDir)) {{
      Move-Item -LiteralPath $BackupDir -Destination $CurrentDir
    }}
    Write-Result "failed" "自动更新失败，已恢复旧版本：$Failure"
    if (Test-Path -LiteralPath $CurrentExe) {{
      Start-Process -FilePath $CurrentExe -WorkingDirectory $CurrentDir | Out-Null
    }}
  }} catch {{
    Write-Log "自动回滚失败：$($_.Exception.Message)"
    Write-Result "failed" "自动更新和回滚均失败，请查看更新日志。"
  }}
}}
"#,
        current_pid = options.current_pid,
        current_dir = powershell_literal(options.current_dir),
        current_exe = powershell_literal(options.current_exe),
        target_dir = powershell_literal(options.target_dir),
        staging_dir = powershell_literal(&staging_dir),
        zip_path = powershell_literal(options.zip_path),
        log_path = powershell_literal(options.log_path),
        result_path = powershell_literal(options.result_path),
        version = options.version.replace('\'', "''"),
        portable_exe = PORTABLE_EXE_NAME,
    )
}

fn powershell_literal(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

fn write_utf8_bom(path: &Path, content: &str) -> Result<(), String> {
    let mut file = File::create(path).map_err(to_string)?;
    file.write_all(&[0xEF, 0xBB, 0xBF]).map_err(to_string)?;
    file.write_all(content.as_bytes()).map_err(to_string)?;
    file.flush().map_err(to_string)
}

fn spawn_updater(script_path: &Path) -> Result<(), String> {
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(script_path)
        .current_dir(
            script_path
                .parent()
                .ok_or_else(|| "更新器脚本目录无效。".to_string())?,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.spawn().map_err(to_string)?;
    Ok(())
}

fn ensure_update_can_start(state: &State<'_, AppState>) -> Result<(), String> {
    if state.active_tasks.any_active()? || state.single_rewrite_tasks.any_active()? {
        return Err("当前有分析、改写或单章重写任务正在运行，请等待任务结束后再更新。".to_string());
    }
    Ok(())
}

pub(crate) fn release_tag_from_url(url: &str) -> Option<String> {
    let clean_url = url.split(['?', '#']).next().unwrap_or(url);
    let segments = clean_url
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();
    segments
        .windows(2)
        .find(|pair| pair[0] == "tag")
        .map(|pair| pair[1].to_string())
}

pub(crate) fn portable_zip_name(version: &str) -> String {
    format!(
        "YuriRewrite-v{}-windows-x64.zip",
        normalize_release_version(version)
    )
}

pub(crate) fn normalize_release_version(version: &str) -> String {
    version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

pub(crate) fn is_newer_version(candidate: &str, current: &str) -> bool {
    let candidate_parts = version_number_parts(candidate);
    let current_parts = version_number_parts(current);
    let max_len = candidate_parts.len().max(current_parts.len()).max(1);
    for idx in 0..max_len {
        let left = *candidate_parts.get(idx).unwrap_or(&0);
        let right = *current_parts.get(idx).unwrap_or(&0);
        if left != right {
            return left > right;
        }
    }
    false
}

pub(crate) fn version_number_parts(version: &str) -> Vec<u64> {
    normalize_release_version(version)
        .split(['.', '-', '+'])
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

pub(crate) fn default_download_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|home| home.join("Downloads"))
        .filter(|path| path.exists())
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join("Downloads"))
                .filter(|path| path.exists())
        })
}

pub(crate) fn open_url_in_default_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let status = Command::new("cmd")
        .args(["/C", "start", "", url])
        .status()
        .map_err(to_string)?;

    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status().map_err(to_string)?;

    #[cfg(all(unix, not(target_os = "macos")))]
    let status = Command::new("xdg-open")
        .arg(url)
        .status()
        .map_err(to_string)?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("无法打开链接：{}", url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn write_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).expect("create zip");
        let mut writer = ZipWriter::new(file);
        for (name, content) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .expect("start file");
            writer.write_all(content).expect("write file");
        }
        writer.finish().expect("finish zip");
    }

    #[test]
    fn update_sources_keep_github_then_fixed_mirrors() {
        let original = "https://github.com/x/y/releases/download/v1/a.zip";
        let sources = update_download_sources(original);
        assert_eq!(sources[0].label, "GitHub");
        assert_eq!(sources[0].url, original);
        assert_eq!(
            sources[1].url,
            format!("https://gh-proxy.com/{}", original)
        );
        assert_eq!(
            sources[2].url,
            format!("https://ghproxy.net/{}", original)
        );
        assert_eq!(
            sources[3].url,
            format!("https://ghfast.top/{}", original)
        );
    }

    #[test]
    fn sha256_digest_normalization_is_strict() {
        let digest = "A".repeat(64);
        assert_eq!(
            normalize_sha256_digest(&format!("sha256:{}", digest)),
            Some("a".repeat(64))
        );
        assert_eq!(normalize_sha256_digest("sha256:1234"), None);
        assert_eq!(normalize_sha256_digest(&"z".repeat(64)), None);
    }

    #[test]
    fn release_version_rejects_path_characters() {
        assert_eq!(safe_release_version("v0.3.11").as_deref(), Ok("0.3.11"));
        assert!(safe_release_version("../0.3.11").is_err());
        assert!(safe_release_version(r"0.3.11\bad").is_err());
    }

    #[test]
    fn portable_zip_validation_accepts_exact_release_layout() {
        let dir = env::temp_dir().join(format!("yuri-update-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp");
        let zip_path = dir.join("valid.zip");
        write_test_zip(
            &zip_path,
            &[
                (PORTABLE_EXE_NAME, b"exe"),
                (PORTABLE_README_NAME, b"readme"),
            ],
        );
        assert!(validate_portable_zip(&zip_path).is_ok());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn portable_zip_validation_rejects_extra_or_nested_files() {
        let dir = env::temp_dir().join(format!("yuri-update-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp");
        let extra_zip = dir.join("extra.zip");
        write_test_zip(
            &extra_zip,
            &[
                (PORTABLE_EXE_NAME, b"exe"),
                (PORTABLE_README_NAME, b"readme"),
                ("install.ps1", b"bad"),
            ],
        );
        assert!(validate_portable_zip(&extra_zip).is_err());

        let nested_zip = dir.join("nested.zip");
        write_test_zip(
            &nested_zip,
            &[
                ("folder/Yuri Rewrite.exe", b"exe"),
                (PORTABLE_README_NAME, b"readme"),
            ],
        );
        assert!(validate_portable_zip(&nested_zip).is_err());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn sha256_verification_rejects_mismatch() {
        let dir = env::temp_dir().join(format!("yuri-update-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp");
        let path = dir.join("asset.zip");
        fs::write(&path, b"content").expect("write");
        assert!(verify_sha256(&path, &"0".repeat(64)).is_err());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn updater_script_contains_backup_rollback_and_restart_steps() {
        let root = PathBuf::from(r"C:\Apps");
        let current_dir = root.join("YuriRewrite-v0.3.10-windows-x64");
        let current_exe = current_dir.join(PORTABLE_EXE_NAME);
        let target_dir = root.join("YuriRewrite-v0.3.11-windows-x64");
        let script = build_updater_script(UpdaterScriptOptions {
            current_pid: 123,
            current_dir: &current_dir,
            current_exe: &current_exe,
            target_dir: &target_dir,
            zip_path: Path::new(r"C:\Temp\update.zip"),
            log_path: Path::new(r"C:\Temp\update.log"),
            result_path: Path::new(r"C:\Temp\result.json"),
            version: "0.3.11",
        });
        assert!(script.contains("Wait-Process -Id $CurrentPid"));
        assert!(script.contains("$CurrentDir.old-$Timestamp"));
        assert!(script.contains("Expand-Archive"));
        assert!(script.contains("Move-Item -LiteralPath $BackupDir -Destination $CurrentDir"));
        assert!(script.contains("Start-Process -FilePath $NewExe"));
    }

    #[test]
    fn updater_script_escapes_single_quotes_in_paths() {
        let path = PathBuf::from(r"C:\User's Apps\YuriRewrite-v0.3.10-windows-x64");
        let script = build_updater_script(UpdaterScriptOptions {
            current_pid: 1,
            current_dir: &path,
            current_exe: &path.join(PORTABLE_EXE_NAME),
            target_dir: &PathBuf::from(r"C:\User's Apps\YuriRewrite-v0.3.11-windows-x64"),
            zip_path: Path::new(r"C:\Temp\update.zip"),
            log_path: Path::new(r"C:\Temp\update.log"),
            result_path: Path::new(r"C:\Temp\result.json"),
            version: "0.3.11",
        });
        assert!(script.contains(r"$CurrentDir = 'C:\User''s Apps"));
    }

    #[test]
    fn updater_script_is_written_with_utf8_bom_for_windows_powershell() {
        let dir = env::temp_dir().join(format!("yuri-update-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp");
        let path = dir.join("apply-update.ps1");
        write_utf8_bom(&path, "Write-Host '中文路径'").expect("write script");
        let bytes = fs::read(&path).expect("read script");
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
        assert!(String::from_utf8_lossy(&bytes[3..]).contains("中文路径"));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn zip_reader_rejects_parent_traversal() {
        let buffer = Cursor::new(Vec::<u8>::new());
        let mut writer = ZipWriter::new(buffer);
        writer
            .start_file("../Yuri Rewrite.exe", SimpleFileOptions::default())
            .expect("start");
        writer.write_all(b"exe").expect("write");
        let cursor = writer.finish().expect("finish");
        let dir = env::temp_dir().join(format!("yuri-update-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp");
        let path = dir.join("traversal.zip");
        fs::write(&path, cursor.into_inner()).expect("write zip");
        assert!(validate_portable_zip(&path).is_err());
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
