use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri_plugin_dialog::FilePath;

#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallRequest {
    source_path: String,
    install_dir: String,
    package_kind: PackageKind,
    preview_root: Option<String>,
    selected_executable_path: Option<String>,
    desktop_config: DesktopConfig,
    create_desktop_entry: bool,
    add_to_path: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopConfig {
    name: String,
    icon_path: Option<String>,
    terminal: bool,
    categories: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum PackageKind {
    AppImage,
    Tarball,
    Archive,
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallResponse {
    app_name: String,
    install_root: String,
    executable_path: Option<String>,
    desktop_entry_path: Option<String>,
    path_link: Option<String>,
    steps: Vec<InstallStep>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledAppsRequest {
    install_dir: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UninstallRequest {
    install_dir: String,
    app_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstalledApp {
    app_name: String,
    install_root: String,
    executable_path: Option<String>,
    desktop_entry_path: Option<String>,
    path_link: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UninstallResponse {
    app_name: String,
    steps: Vec<InstallStep>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewRequest {
    source_path: String,
    package_kind: PackageKind,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewResponse {
    preview_root: String,
    executables: Vec<ExecutableCandidate>,
    icon_candidates: Vec<IconCandidate>,
    desktop_suggestion: DesktopSuggestion,
    steps: Vec<InstallStep>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceFileInfo {
    file_name: String,
    file_size: u64,
    source_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecutableCandidate {
    path: String,
    relative_path: String,
    file_name: String,
    score: u8,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IconCandidate {
    path: String,
    relative_path: String,
    file_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopSuggestion {
    name: String,
    icon_path: Option<String>,
    icon_relative_path: Option<String>,
    terminal: bool,
    categories: String,
}

#[derive(Default)]
struct DesktopMetadata {
    name: Option<String>,
    icon: Option<String>,
    terminal: Option<bool>,
    categories: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallStep {
    title: String,
    status: StepStatus,
    detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum StepStatus {
    Done,
    Skipped,
}

#[tauri::command]
fn inspect_source_file(source_path: String) -> Result<SourceFileInfo, String> {
    let source_path = parse_user_path(&source_path)?;
    inspect_source_path(&source_path)
}

fn inspect_source_path(source_path: &Path) -> Result<SourceFileInfo, String> {
    if !source_path.is_absolute() {
        return Err("未获取到真实文件路径，请使用系统文件选择器导入文件".into());
    }

    let metadata =
        fs::metadata(&source_path).map_err(|error| format!("读取安装文件失败：{}", error))?;

    if !metadata.is_file() {
        return Err("选择的路径不是文件".into());
    }

    let file_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("application")
        .to_string();

    Ok(SourceFileInfo {
        file_name,
        file_size: metadata.len(),
        source_path: source_path.display().to_string(),
    })
}

#[tauri::command]
fn preview_package(request: PreviewRequest) -> Result<PreviewResponse, String> {
    let source_path = parse_user_path(&request.source_path)?;

    validate_source(&source_path, request.package_kind)?;

    let app_name = app_name_from_path(&source_path);
    let mut preview_root = create_preview_dir(&app_name)?;
    let mut steps = Vec::new();

    match request.package_kind {
        PackageKind::AppImage => {
            preview_root = extract_appimage(&source_path, &preview_root)?;
            steps.push(done_step(
                "临时解压",
                format!("已解包 AppImage 到 {}", preview_root.display()),
            ));
        }
        PackageKind::Tarball | PackageKind::Archive => {
            extract_archive(&source_path, request.package_kind, &preview_root)?;
            steps.push(done_step(
                "临时解压",
                format!("已解压到 {}", preview_root.display()),
            ));
        }
        PackageKind::Unknown => return Err("暂不支持该文件类型".into()),
    }

    let executables = if request.package_kind == PackageKind::AppImage {
        vec![executable_candidate(
            &source_path,
            source_path.parent().unwrap_or(Path::new("")),
        )?]
    } else {
        find_executables(&preview_root)
            .into_iter()
            .map(|path| executable_candidate(&path, &preview_root))
            .collect::<Result<Vec<_>, _>>()?
    };

    if executables.is_empty() {
        return Err("已完成临时解压，但未找到可执行文件".into());
    }

    let icon_candidates = find_icon_candidates(&preview_root)
        .into_iter()
        .map(|path| icon_candidate(&path, &preview_root))
        .collect::<Result<Vec<_>, _>>()?;
    let desktop_suggestion = read_desktop_suggestion(&preview_root, &app_name, &icon_candidates);

    steps.push(done_step(
        "查找入口",
        format!("检测到 {} 个可执行文件", executables.len()),
    ));

    if !icon_candidates.is_empty() {
        steps.push(done_step(
            "读取图标",
            format!("检测到 {} 个图标文件", icon_candidates.len()),
        ));
    }

    Ok(PreviewResponse {
        preview_root: preview_root.display().to_string(),
        executables,
        icon_candidates,
        desktop_suggestion,
        steps,
    })
}

#[tauri::command]
fn install_package(request: InstallRequest) -> Result<InstallResponse, String> {
    let source_path = parse_user_path(&request.source_path)?;
    let install_dir = expand_home(request.install_dir.trim())?;

    validate_source(&source_path, request.package_kind)?;

    if install_dir.as_os_str().is_empty() {
        return Err("安装目录不能为空".into());
    }
    if !install_dir.is_absolute() {
        return Err("安装目录必须是绝对路径，或使用 ~/ 开头的路径".into());
    }

    let app_name = app_name_from_path(&source_path);
    let install_root = install_dir.join(&app_name);
    let mut steps = Vec::new();

    fs::create_dir_all(&install_root).map_err(|error| format!("创建安装目录失败：{}", error))?;
    steps.push(done_step(
        "准备目录",
        format!("已创建 {}", install_root.display()),
    ));

    let executable_path = match request.package_kind {
        PackageKind::AppImage => install_appimage(&source_path, &install_root, &mut steps)?,
        PackageKind::Tarball | PackageKind::Archive => install_archive(
            request.preview_root.as_deref(),
            request.selected_executable_path.as_deref(),
            &install_root,
            &mut steps,
        )?,
        PackageKind::Unknown => return Err("暂不支持该文件类型".into()),
    };

    let desktop_entry_path = if request.create_desktop_entry {
        let installed_icon_path = resolve_installed_icon_path(
            request.desktop_config.icon_path.as_deref(),
            request.preview_root.as_deref(),
            &install_root,
            request.package_kind,
        )?;

        Some(create_desktop_entry(
            &request.desktop_config,
            &app_name,
            &executable_path,
            request.package_kind,
            installed_icon_path.as_deref(),
            &mut steps,
        )?)
    } else {
        steps.push(skipped_step("桌面入口", "已按选项跳过创建 .desktop 文件"));
        None
    };

    let path_link = if request.add_to_path {
        Some(link_to_local_bin(&app_name, &executable_path, &mut steps)?)
    } else {
        steps.push(skipped_step("PATH 链接", "已按选项跳过命令行链接"));
        None
    };

    Ok(InstallResponse {
        app_name,
        install_root: install_root.display().to_string(),
        executable_path: Some(executable_path.display().to_string()),
        desktop_entry_path,
        path_link,
        steps,
    })
}

#[tauri::command]
fn list_installed_apps(request: InstalledAppsRequest) -> Result<Vec<InstalledApp>, String> {
    let install_dir = validate_install_dir(&request.install_dir)?;

    if !install_dir.exists() {
        return Ok(Vec::new());
    }

    let mut apps = Vec::new();
    for entry in
        fs::read_dir(&install_dir).map_err(|error| format!("读取安装目录失败：{}", error))?
    {
        let entry = entry.map_err(|error| format!("读取安装目录项失败：{}", error))?;
        let metadata = entry
            .metadata()
            .map_err(|error| format!("读取安装目录项信息失败：{}", error))?;

        if !metadata.is_dir() {
            continue;
        }

        let app_name = entry.file_name().to_string_lossy().to_string();
        let install_root = entry.path();
        apps.push(InstalledApp {
            app_name,
            install_root: install_root.display().to_string(),
            executable_path: find_executables(&install_root)
                .first()
                .map(|path| path.display().to_string()),
            desktop_entry_path: find_desktop_entry_for_install_root(&install_root)
                .map(|path| path.display().to_string()),
            path_link: find_path_link_for_install_root(&install_root)
                .map(|path| path.display().to_string()),
        });
    }

    apps.sort_by(|left, right| left.app_name.cmp(&right.app_name));
    Ok(apps)
}

#[tauri::command]
fn uninstall_app(request: UninstallRequest) -> Result<UninstallResponse, String> {
    let install_dir = validate_install_dir(&request.install_dir)?;
    let app_name = sanitize_file_stem(request.app_name.trim());
    let install_root = install_dir.join(&app_name);
    let canonical_install_dir = install_dir
        .canonicalize()
        .map_err(|error| format!("读取安装目录失败：{}", error))?;
    let canonical_install_root = install_root
        .canonicalize()
        .map_err(|_| "该应用不在当前安装目录中".to_string())?;

    if !canonical_install_root.starts_with(&canonical_install_dir) {
        return Err("应用目录不在当前安装目录中，已停止卸载".into());
    }

    let mut steps = Vec::new();

    if let Some(desktop_entry_path) = find_desktop_entry_for_install_root(&canonical_install_root) {
        fs::remove_file(&desktop_entry_path)
            .map_err(|error| format!("删除桌面入口失败：{}", error))?;
        steps.push(done_step(
            "桌面入口",
            format!("已删除 {}", desktop_entry_path.display()),
        ));
    } else {
        steps.push(skipped_step("桌面入口", "未找到指向该应用的 .desktop 文件"));
    }

    if let Some(path_link) = find_path_link_for_install_root(&canonical_install_root) {
        fs::remove_file(&path_link).map_err(|error| format!("删除 PATH 链接失败：{}", error))?;
        steps.push(done_step(
            "PATH 链接",
            format!("已删除 {}", path_link.display()),
        ));
    } else {
        steps.push(skipped_step("PATH 链接", "未找到指向该应用的命令行链接"));
    }

    fs::remove_dir_all(&canonical_install_root)
        .map_err(|error| format!("删除应用目录失败：{}", error))?;
    steps.push(done_step(
        "应用文件",
        format!("已删除 {}", canonical_install_root.display()),
    ));

    Ok(UninstallResponse { app_name, steps })
}

fn validate_install_dir(value: &str) -> Result<PathBuf, String> {
    let install_dir = expand_home(value.trim())?;

    if install_dir.as_os_str().is_empty() {
        return Err("安装目录不能为空".into());
    }

    if !install_dir.is_absolute() {
        return Err("安装目录必须是绝对路径，或使用 ~/ 开头的路径".into());
    }

    Ok(install_dir)
}

fn validate_source(source_path: &Path, package_kind: PackageKind) -> Result<(), String> {
    if package_kind == PackageKind::Unknown {
        return Err("请选择 AppImage、tar.* 或 zip 安装包".into());
    }

    if !source_path.is_absolute() {
        return Err("未获取到真实文件路径，请在 Tauri 桌面窗口中选择文件".into());
    }

    let metadata =
        fs::metadata(source_path).map_err(|error| format!("读取安装文件失败：{}", error))?;

    if !metadata.is_file() {
        return Err("选择的路径不是文件".into());
    }

    Ok(())
}

fn parse_user_path(value: &str) -> Result<PathBuf, String> {
    FilePath::from_str(value.trim())
        .map_err(|error| format!("解析文件路径失败：{}", error))?
        .into_path()
        .map_err(|error| format!("解析文件路径失败：{}", error))
}

fn install_appimage(
    source_path: &Path,
    install_root: &Path,
    steps: &mut Vec<InstallStep>,
) -> Result<PathBuf, String> {
    let file_name = source_path
        .file_name()
        .ok_or_else(|| "安装文件名无效".to_string())?;
    let executable_path = install_root.join(file_name);

    fs::copy(source_path, &executable_path)
        .map_err(|error| format!("复制 AppImage 失败：{}", error))?;
    steps.push(done_step(
        "复制文件",
        format!("已复制到 {}", executable_path.display()),
    ));

    make_executable(&executable_path)?;
    steps.push(done_step("执行权限", "已设置为可执行"));

    Ok(executable_path)
}

fn extract_appimage(source_path: &Path, preview_parent: &Path) -> Result<PathBuf, String> {
    let file_name = source_path
        .file_name()
        .ok_or_else(|| "AppImage 文件名无效".to_string())?;
    let extractor_path = preview_parent.join(file_name);

    fs::copy(source_path, &extractor_path)
        .map_err(|error| format!("准备 AppImage 解包文件失败：{}", error))?;
    make_executable(&extractor_path)?;

    let status = Command::new(&extractor_path)
        .arg("--appimage-extract")
        .current_dir(preview_parent)
        .env("APPIMAGELAUNCHER_DISABLE", "1")
        .status()
        .map_err(|error| format!("执行 AppImage 解包失败：{}", error))?;

    if !status.success() {
        return Err("AppImage 解包失败，请确认文件有效且系统支持运行该 AppImage".into());
    }

    let extracted_root = preview_parent.join("squashfs-root");
    if extracted_root.is_dir() {
        Ok(extracted_root)
    } else {
        Err("AppImage 解包完成，但未找到 squashfs-root 目录".into())
    }
}

fn install_archive(
    preview_root: Option<&str>,
    selected_executable_path: Option<&str>,
    install_root: &Path,
    steps: &mut Vec<InstallStep>,
) -> Result<PathBuf, String> {
    let preview_root = preview_root
        .map(|value| PathBuf::from(value.trim()))
        .ok_or_else(|| "请先导入并确认可执行文件".to_string())?;
    let selected_executable_path = selected_executable_path
        .map(|value| PathBuf::from(value.trim()))
        .ok_or_else(|| "请先选择要安装的可执行文件".to_string())?;

    let (preview_root, selected_executable_path) =
        validate_preview_selection(&preview_root, &selected_executable_path)?;
    copy_dir_contents(&preview_root, install_root)?;
    steps.push(done_step(
        "复制文件",
        format!("已从临时目录复制到 {}", install_root.display()),
    ));

    let relative_executable_path = selected_executable_path
        .strip_prefix(&preview_root)
        .map_err(|_| "可执行文件不在临时解压目录中".to_string())?;
    let executable_path = install_root.join(relative_executable_path);
    make_executable(&executable_path)?;
    steps.push(done_step(
        "确认入口",
        format!("使用 {}", executable_path.display()),
    ));

    Ok(executable_path)
}

fn extract_archive(
    source_path: &Path,
    package_kind: PackageKind,
    install_root: &Path,
) -> Result<(), String> {
    let status = match package_kind {
        PackageKind::Tarball => {
            let flag =
                if path_ends_with(source_path, ".tar.gz") || path_ends_with(source_path, ".tgz") {
                    "-xzf"
                } else if path_ends_with(source_path, ".tar.xz") {
                    "-xJf"
                } else if path_ends_with(source_path, ".tar.bz2") {
                    "-xjf"
                } else {
                    return Err("暂不支持该 tar 格式".into());
                };

            Command::new("tar")
                .arg(flag)
                .arg(source_path)
                .arg("-C")
                .arg(install_root)
                .status()
                .map_err(|error| format!("执行 tar 失败：{}", error))?
        }
        PackageKind::Archive => Command::new("unzip")
            .arg("-q")
            .arg(source_path)
            .arg("-d")
            .arg(install_root)
            .status()
            .map_err(|error| format!("执行 unzip 失败：{}", error))?,
        PackageKind::AppImage | PackageKind::Unknown => {
            return Err("该类型不需要解压".into());
        }
    };

    if status.success() {
        Ok(())
    } else {
        Err("解压命令执行失败，请确认系统已安装 tar/unzip 且压缩包有效".into())
    }
}

fn create_desktop_entry(
    config: &DesktopConfig,
    app_name: &str,
    executable_path: &Path,
    package_kind: PackageKind,
    icon_path: Option<&Path>,
    steps: &mut Vec<InstallStep>,
) -> Result<String, String> {
    let desktop_dir = home_dir()?.join(".local/share/applications");
    fs::create_dir_all(&desktop_dir).map_err(|error| format!("创建桌面入口目录失败：{}", error))?;

    let desktop_name = clean_desktop_value(&config.name);
    let display_name = if desktop_name.is_empty() {
        app_name.to_string()
    } else {
        desktop_name
    };
    let categories = normalize_categories(&config.categories);
    let desktop_path = desktop_dir.join(format!("{}.desktop", sanitize_file_stem(&display_name)));
    let mut content = format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={}\nTerminal={}\nCategories={}\n",
        display_name,
        desktop_exec_command(executable_path, package_kind),
        if config.terminal { "true" } else { "false" },
        categories
    );

    if let Some(icon_path) = icon_path {
        content.push_str(&format!(
            "Icon={}\n",
            clean_desktop_value(&icon_path.display().to_string())
        ));
    }

    fs::write(&desktop_path, content).map_err(|error| format!("写入桌面入口失败：{}", error))?;
    steps.push(done_step(
        "桌面入口",
        format!("已创建 {}", desktop_path.display()),
    ));

    Ok(desktop_path.display().to_string())
}

fn read_desktop_suggestion(
    root: &Path,
    fallback_name: &str,
    icons: &[IconCandidate],
) -> DesktopSuggestion {
    let metadata = find_desktop_files(root)
        .first()
        .and_then(|path| parse_desktop_metadata(path).ok())
        .unwrap_or_default();
    let icon_path = metadata
        .icon
        .as_deref()
        .and_then(|value| resolve_icon_value(root, value, icons))
        .or_else(|| icons.first().map(|icon| icon.path.clone()));
    let icon_relative_path = icon_path.as_deref().and_then(|path| {
        Path::new(path)
            .strip_prefix(root)
            .ok()
            .and_then(|value| value.to_str())
            .map(|value| value.to_string())
    });

    DesktopSuggestion {
        name: metadata.name.unwrap_or_else(|| fallback_name.to_string()),
        icon_path,
        icon_relative_path,
        terminal: metadata.terminal.unwrap_or(false),
        categories: metadata.categories.unwrap_or_else(|| "Utility;".into()),
    }
}

fn find_desktop_files(root: &Path) -> Vec<PathBuf> {
    find_files_by_extension(root, &["desktop"][..])
}

fn find_icon_candidates(root: &Path) -> Vec<PathBuf> {
    let mut icons = find_files_by_extension(root, &["png", "svg", "xpm", "ico"][..]);
    icons.sort_by_key(|path| {
        let value = path.display().to_string().to_lowercase();
        (
            if value.contains("/apps/") { 0 } else { 1 },
            if value.contains("256") || value.contains("512") {
                0
            } else if value.contains("128") {
                1
            } else {
                2
            },
            value,
        )
    });
    icons.truncate(50);
    icons
}

fn find_files_by_extension(root: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };

            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file()
                && path
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|extension| {
                        extensions
                            .iter()
                            .any(|accepted| extension.eq_ignore_ascii_case(accepted))
                    })
                    .unwrap_or(false)
            {
                files.push(path);
            }
        }
    }

    files.sort();
    files
}

fn icon_candidate(path: &Path, root: &Path) -> Result<IconCandidate, String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("icon")
        .to_string();
    let relative_path = path
        .strip_prefix(root)
        .ok()
        .and_then(|value| value.to_str())
        .unwrap_or(&file_name)
        .to_string();

    Ok(IconCandidate {
        path: path.display().to_string(),
        relative_path,
        file_name,
    })
}

fn parse_desktop_metadata(path: &Path) -> Result<DesktopMetadata, String> {
    let content =
        fs::read_to_string(path).map_err(|error| format!("读取 desktop 文件失败：{}", error))?;
    let mut metadata = DesktopMetadata::default();
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line.eq_ignore_ascii_case("[Desktop Entry]");
            continue;
        }

        if !in_desktop_entry {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = clean_desktop_value(value);

        match key.trim() {
            "Name" if metadata.name.is_none() => metadata.name = Some(value),
            "Icon" if metadata.icon.is_none() => metadata.icon = Some(value),
            "Terminal" if metadata.terminal.is_none() => {
                metadata.terminal = Some(value.eq_ignore_ascii_case("true"))
            }
            "Categories" if metadata.categories.is_none() => metadata.categories = Some(value),
            _ => {}
        }
    }

    Ok(metadata)
}

fn resolve_icon_value(root: &Path, value: &str, icons: &[IconCandidate]) -> Option<String> {
    if value.trim().is_empty() {
        return None;
    }

    let icon_path = PathBuf::from(value);
    if icon_path.is_absolute() && icon_path.exists() {
        return Some(icon_path.display().to_string());
    }

    let rooted_path = root.join(value);
    if rooted_path.exists() {
        return Some(rooted_path.display().to_string());
    }

    let requested_stem = Path::new(value)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(value);

    icons
        .iter()
        .find(|icon| {
            Path::new(&icon.file_name)
                .file_stem()
                .and_then(|name| name.to_str())
                .map(|name| name == requested_stem)
                .unwrap_or(false)
        })
        .map(|icon| icon.path.clone())
}

fn resolve_installed_icon_path(
    icon_path: Option<&str>,
    preview_root: Option<&str>,
    install_root: &Path,
    package_kind: PackageKind,
) -> Result<Option<PathBuf>, String> {
    let Some(icon_path) = icon_path.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let icon_path = PathBuf::from(icon_path);
    let installed_path = if package_kind == PackageKind::AppImage {
        let canonical_icon_path = icon_path
            .canonicalize()
            .map_err(|_| "选择的桌面图标不存在，请重新导入或选择默认图标".to_string())?;

        if let Some(preview_root) = preview_root {
            let preview_root = PathBuf::from(preview_root.trim())
                .canonicalize()
                .map_err(|_| "临时解压目录不存在，请重新导入文件".to_string())?;

            if canonical_icon_path.starts_with(&preview_root) {
                copy_icon_to_install_root(&canonical_icon_path, install_root)?
            } else {
                canonical_icon_path
            }
        } else {
            canonical_icon_path
        }
    } else if let Some(preview_root) = preview_root {
        let preview_root = PathBuf::from(preview_root.trim())
            .canonicalize()
            .map_err(|_| "临时解压目录不存在，请重新导入文件".to_string())?;
        let canonical_icon_path = icon_path
            .canonicalize()
            .map_err(|_| "选择的桌面图标不存在，请重新导入或选择默认图标".to_string())?;

        if canonical_icon_path.starts_with(&preview_root) {
            let relative_path = canonical_icon_path
                .strip_prefix(&preview_root)
                .map_err(|_| "图标文件不在临时解压目录中".to_string())?;
            install_root.join(relative_path)
        } else {
            canonical_icon_path
        }
    } else {
        icon_path
    };

    if installed_path.exists() {
        Ok(Some(installed_path))
    } else {
        Err("选择的桌面图标不存在，请重新导入或选择默认图标".into())
    }
}

fn copy_icon_to_install_root(icon_path: &Path, install_root: &Path) -> Result<PathBuf, String> {
    let file_name = icon_path
        .file_name()
        .ok_or_else(|| "选择的桌面图标文件名无效".to_string())?;
    let icon_dir = install_root.join("icons");
    fs::create_dir_all(&icon_dir).map_err(|error| format!("创建图标目录失败：{}", error))?;

    let installed_icon_path = icon_dir.join(file_name);
    fs::copy(icon_path, &installed_icon_path)
        .map_err(|error| format!("复制桌面图标失败：{}", error))?;

    Ok(installed_icon_path)
}

fn clean_desktop_value(value: &str) -> String {
    value
        .chars()
        .filter(|character| !matches!(character, '\n' | '\r'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn normalize_categories(value: &str) -> String {
    let cleaned = clean_desktop_value(value);
    let categories = if cleaned.is_empty() {
        "Utility".to_string()
    } else {
        cleaned
    };

    if categories.ends_with(';') {
        categories
    } else {
        format!("{};", categories)
    }
}

fn link_to_local_bin(
    app_name: &str,
    executable_path: &Path,
    steps: &mut Vec<InstallStep>,
) -> Result<String, String> {
    let bin_dir = home_dir()?.join(".local/bin");
    fs::create_dir_all(&bin_dir).map_err(|error| format!("创建 PATH 目录失败：{}", error))?;

    let link_path = bin_dir.join(sanitize_file_stem(app_name));

    if link_path.exists() {
        fs::remove_file(&link_path)
            .map_err(|error| format!("替换已有 PATH 链接失败：{}", error))?;
    }

    #[cfg(unix)]
    {
        symlink(executable_path, &link_path)
            .map_err(|error| format!("创建 PATH 链接失败：{}", error))?;
    }

    #[cfg(not(unix))]
    {
        fs::copy(executable_path, &link_path)
            .map_err(|error| format!("创建 PATH 副本失败：{}", error))?;
    }

    steps.push(done_step(
        "PATH 链接",
        format!("已链接到 {}", link_path.display()),
    ));

    Ok(link_path.display().to_string())
}

fn find_desktop_entry_for_install_root(install_root: &Path) -> Option<PathBuf> {
    let desktop_dir = home_dir().ok()?.join(".local/share/applications");
    let entries = fs::read_dir(desktop_dir).ok()?;
    let install_root = install_root
        .canonicalize()
        .unwrap_or_else(|_| install_root.to_path_buf());

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if content
            .lines()
            .filter_map(|line| line.trim().strip_prefix("Exec="))
            .any(|value| desktop_exec_references_root(value, &install_root))
        {
            return Some(path);
        }
    }

    None
}

fn desktop_exec_references_root(value: &str, install_root: &Path) -> bool {
    let install_root = install_root.display().to_string();
    let cleaned = value.replace("\\\"", "\"");
    cleaned.contains(&install_root)
}

fn find_path_link_for_install_root(install_root: &Path) -> Option<PathBuf> {
    let bin_dir = home_dir().ok()?.join(".local/bin");
    let entries = fs::read_dir(bin_dir).ok()?;
    let install_root = install_root
        .canonicalize()
        .unwrap_or_else(|_| install_root.to_path_buf());

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };

        if !metadata.file_type().is_symlink() {
            continue;
        }

        let Ok(target) = fs::read_link(&path) else {
            continue;
        };
        let resolved_target = if target.is_absolute() {
            target
        } else {
            path.parent().unwrap_or(Path::new("")).join(target)
        };
        let canonical_target = resolved_target
            .canonicalize()
            .unwrap_or_else(|_| resolved_target.clone());

        if canonical_target.starts_with(&install_root) {
            return Some(path);
        }
    }

    None
}

fn find_executables(root: &Path) -> Vec<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut candidates = Vec::new();

    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };

            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() && is_executable(&metadata) {
                candidates.push(path);
            }
        }
    }

    candidates.sort_by_key(|path| {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        (candidate_score(path), name.to_string())
    });

    candidates
}

fn executable_candidate(path: &Path, root: &Path) -> Result<ExecutableCandidate, String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("executable")
        .to_string();
    let relative_path = path
        .strip_prefix(root)
        .ok()
        .and_then(|value| value.to_str())
        .unwrap_or(&file_name)
        .to_string();

    Ok(ExecutableCandidate {
        path: path.display().to_string(),
        relative_path,
        file_name,
        score: candidate_score(path),
    })
}

fn candidate_score(path: &Path) -> u8 {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase();

    if name.ends_with(".desktop") {
        3
    } else if name.ends_with(".sh") {
        2
    } else if path
        .components()
        .any(|component| component.as_os_str().to_string_lossy() == "bin")
    {
        0
    } else {
        1
    }
}

fn create_preview_dir(app_name: &str) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("生成临时目录失败：{}", error))?
        .as_millis();
    let preview_root = env::temp_dir().join(format!(
        "auto-installer-{}-{}-{}",
        sanitize_file_stem(app_name),
        std::process::id(),
        timestamp
    ));

    fs::create_dir_all(&preview_root).map_err(|error| format!("创建临时目录失败：{}", error))?;
    Ok(preview_root)
}

fn validate_preview_selection(
    preview_root: &Path,
    selected_path: &Path,
) -> Result<(PathBuf, PathBuf), String> {
    if !preview_root.is_absolute() || !selected_path.is_absolute() {
        return Err("临时目录和可执行文件路径必须是绝对路径".into());
    }

    let preview_root = preview_root
        .canonicalize()
        .map_err(|_| "临时解压目录不存在，请重新导入文件".to_string())?;
    let selected_path = selected_path
        .canonicalize()
        .map_err(|error| format!("读取可执行文件失败：{}", error))?;

    if !selected_path.starts_with(&preview_root) {
        return Err("选择的可执行文件不在临时解压目录中".into());
    }

    let metadata =
        fs::metadata(&selected_path).map_err(|error| format!("读取可执行文件失败：{}", error))?;
    if !metadata.is_file() {
        return Err("选择的入口不是文件".into());
    }

    Ok((preview_root, selected_path))
}

fn copy_dir_contents(source: &Path, destination: &Path) -> Result<(), String> {
    let source_root = source
        .canonicalize()
        .map_err(|error| format!("读取临时目录失败：{}", error))?;
    copy_dir_contents_checked(&source_root, destination, &source_root, destination)
}

fn copy_dir_contents_checked(
    source: &Path,
    destination: &Path,
    source_root: &Path,
    destination_root: &Path,
) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| format!("创建安装目录失败：{}", error))?;

    for entry in fs::read_dir(source).map_err(|error| format!("读取临时目录失败：{}", error))?
    {
        let entry = entry.map_err(|error| format!("读取临时目录项失败：{}", error))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取临时目录项类型失败：{}", error))?;
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| format!("读取临时目录项信息失败：{}", error))?;

        if file_type.is_symlink() {
            copy_internal_symlink(
                &source_path,
                &destination_path,
                source_root,
                destination_root,
            )?;
        } else if metadata.is_dir() {
            copy_dir_contents_checked(
                &source_path,
                &destination_path,
                source_root,
                destination_root,
            )?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|error| format!("复制安装文件失败：{}", error))?;
            fs::set_permissions(&destination_path, metadata.permissions())
                .map_err(|error| format!("复制文件权限失败：{}", error))?;
        }
    }

    Ok(())
}

#[cfg(unix)]
fn copy_internal_symlink(
    source_path: &Path,
    destination_path: &Path,
    source_root: &Path,
    destination_root: &Path,
) -> Result<(), String> {
    let target =
        fs::read_link(source_path).map_err(|error| format!("读取符号链接失败：{}", error))?;
    let resolved_target = if target.is_absolute() {
        target.clone()
    } else {
        source_path.parent().unwrap_or(source_root).join(&target)
    };
    let canonical_target = resolved_target
        .canonicalize()
        .map_err(|error| format!("解析符号链接目标失败：{}", error))?;

    if !canonical_target.starts_with(source_root) {
        return Err("压缩包包含指向解压目录外的符号链接，已停止安装".into());
    }

    let installed_target = if target.is_absolute() {
        let relative_target = canonical_target
            .strip_prefix(source_root)
            .map_err(|_| "符号链接目标不在临时解压目录中".to_string())?;
        destination_root.join(relative_target)
    } else {
        target
    };

    if fs::symlink_metadata(destination_path).is_ok() {
        fs::remove_file(destination_path)
            .map_err(|error| format!("替换符号链接失败：{}", error))?;
    }

    symlink(&installed_target, destination_path)
        .map_err(|error| format!("复制符号链接失败：{}", error))
}

#[cfg(not(unix))]
fn copy_internal_symlink(
    _source_path: &Path,
    _destination_path: &Path,
    _source_root: &Path,
    _destination_root: &Path,
) -> Result<(), String> {
    Err("当前平台暂不支持安装包含符号链接的压缩包".into())
}

fn app_name_from_path(path: &Path) -> String {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("application");
    let lower_name = file_name.to_lowercase();
    let stripped = [
        ".appimage",
        ".tar.gz",
        ".tgz",
        ".tar.xz",
        ".tar.bz2",
        ".zip",
    ]
    .iter()
    .find_map(|suffix| lower_name.strip_suffix(suffix).map(|_| suffix.len()))
    .map(|suffix_len| &file_name[..file_name.len() - suffix_len])
    .unwrap_or(file_name);

    sanitize_file_stem(stripped)
}

fn sanitize_file_stem(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('-');

    if trimmed.is_empty() {
        "application".into()
    } else {
        trimmed.to_string()
    }
}

fn expand_home(value: &str) -> Result<PathBuf, String> {
    if value == "~" {
        return home_dir();
    }

    if let Some(rest) = value.strip_prefix("~/") {
        return Ok(home_dir()?.join(rest));
    }

    Ok(PathBuf::from(value))
}

fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "无法读取 HOME 环境变量".into())
}

fn make_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let metadata = fs::metadata(path).map_err(|error| format!("读取权限失败：{}", error))?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        fs::set_permissions(path, permissions)
            .map_err(|error| format!("设置执行权限失败：{}", error))?;
    }

    Ok(())
}

fn is_executable(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn path_ends_with(path: &Path, suffix: &str) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_lowercase().ends_with(suffix))
        .unwrap_or(false)
}

fn desktop_exec_command(path: &Path, package_kind: PackageKind) -> String {
    let executable = desktop_exec_path(path);

    if package_kind == PackageKind::AppImage {
        format!("env APPIMAGELAUNCHER_DISABLE=1 {}", executable)
    } else {
        executable
    }
}

fn desktop_exec_path(path: &Path) -> String {
    let value = path.display().to_string();

    if !value.contains(char::is_whitespace) && !value.contains('"') && !value.contains('\\') {
        return value;
    }

    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn done_step(title: impl Into<String>, detail: impl Into<String>) -> InstallStep {
    InstallStep {
        title: title.into(),
        status: StepStatus::Done,
        detail: detail.into(),
    }
}

fn skipped_step(title: impl Into<String>, detail: impl Into<String>) -> InstallStep {
    InstallStep {
        title: title.into(),
        status: StepStatus::Skipped,
        detail: detail.into(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            inspect_source_file,
            preview_package,
            install_package,
            list_installed_apps,
            uninstall_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
