import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

type PackageKind = "AppImage" | "Tarball" | "Archive" | "Unknown";

type InstallTarget = {
  fileName: string;
  fileSize: number;
  kind: PackageKind;
  sourcePath: string;
};

const supportedTypes = [".AppImage", ".appimage", ".tar.gz", ".tgz", ".tar.xz", ".tar.bz2", ".zip"];
const packageDialogFilters = [
  {
    name: "安装包",
    extensions: ["AppImage", "appimage", "gz", "tgz", "xz", "bz2", "zip"],
  },
];
const defaultInstallPath = "~/.local/share/auto-installer/apps";

type InstallStepStatus = "done" | "skipped";

type InstallStep = {
  title: string;
  status: InstallStepStatus;
  detail: string;
};

type InstallResponse = {
  appName: string;
  installRoot: string;
  executablePath: string | null;
  desktopEntryPath: string | null;
  pathLink: string | null;
  steps: InstallStep[];
};

type InstalledApp = {
  appName: string;
  installRoot: string;
  executablePath: string | null;
  desktopEntryPath: string | null;
  pathLink: string | null;
};

type UninstallResponse = {
  appName: string;
  steps: InstallStep[];
};

type ExecutableCandidate = {
  path: string;
  relativePath: string;
  fileName: string;
  score: number;
};

type IconCandidate = {
  path: string;
  relativePath: string;
  fileName: string;
};

type DesktopSuggestion = {
  name: string;
  iconPath: string | null;
  iconRelativePath: string | null;
  terminal: boolean;
  categories: string;
};

type PreviewResponse = {
  previewRoot: string;
  executables: ExecutableCandidate[];
  iconCandidates: IconCandidate[];
  desktopSuggestion: DesktopSuggestion;
  steps: InstallStep[];
};

type SourceFileInfo = {
  fileName: string;
  fileSize: number;
  sourcePath: string;
};

type InstallState =
  | { status: "idle" }
  | { status: "running" }
  | { status: "success"; response: InstallResponse }
  | { status: "error"; message: string };

type PreviewState =
  | { status: "idle" }
  | { status: "running" }
  | { status: "success"; response: PreviewResponse }
  | { status: "error"; message: string };

type InstalledAppsState =
  | { status: "idle" }
  | { status: "running" }
  | { status: "success"; apps: InstalledApp[] }
  | { status: "error"; message: string };

type UninstallState =
  | { status: "idle" }
  | { status: "running" }
  | { status: "success"; response: UninstallResponse }
  | { status: "error"; message: string };

function detectPackageKind(fileName: string): PackageKind {
  const normalizedName = fileName.toLowerCase();

  if (normalizedName.endsWith(".appimage")) {
    return "AppImage";
  }

  if (
    normalizedName.endsWith(".tar.gz") ||
    normalizedName.endsWith(".tgz") ||
    normalizedName.endsWith(".tar.xz") ||
    normalizedName.endsWith(".tar.bz2")
  ) {
    return "Tarball";
  }

  if (normalizedName.endsWith(".zip")) {
    return "Archive";
  }

  return "Unknown";
}

function formatFileSize(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  const units = ["KB", "MB", "GB"];
  let size = bytes / 1024;
  let unitIndex = 0;

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }

  return `${size.toFixed(size >= 100 ? 0 : 1)} ${units[unitIndex]}`;
}

function App() {
  const [installTarget, setInstallTarget] = useState<InstallTarget | null>(null);
  const [installPath, setInstallPath] = useState(defaultInstallPath);
  const [createDesktopEntry, setCreateDesktopEntry] = useState(true);
  const [desktopName, setDesktopName] = useState("");
  const [desktopIconPath, setDesktopIconPath] = useState("");
  const [desktopCategories, setDesktopCategories] = useState("Utility;");
  const [desktopTerminal, setDesktopTerminal] = useState(false);
  const [addToPath, setAddToPath] = useState(false);
  const [previewState, setPreviewState] = useState<PreviewState>({ status: "idle" });
  const [selectedExecutablePath, setSelectedExecutablePath] = useState("");
  const [installState, setInstallState] = useState<InstallState>({ status: "idle" });
  const [installedAppsState, setInstalledAppsState] = useState<InstalledAppsState>({
    status: "idle",
  });
  const [selectedUninstallAppName, setSelectedUninstallAppName] = useState("");
  const [uninstallState, setUninstallState] = useState<UninstallState>({ status: "idle" });

  const installPlan = useMemo(() => {
    if (!installTarget) {
      return ["等待选择安装文件", "自动识别包类型", "生成安装计划"];
    }

    if (installTarget.kind === "AppImage") {
      return ["确认 AppImage 入口", "复制到应用目录", createDesktopEntry ? "创建桌面入口" : "跳过桌面入口"];
    }

    if (installTarget.kind === "Tarball") {
      return ["解压到临时目录", "确认可执行文件", createDesktopEntry ? "注册启动入口" : "保留本地入口"];
    }

    if (installTarget.kind === "Archive") {
      return ["解压到临时目录", "检查目录结构", "确认可执行入口"];
    }

    return ["记录文件信息", "等待手动指定安装方式", "跳过自动安装"];
  }, [createDesktopEntry, installTarget]);

  const selectedExecutable = useMemo(() => {
    if (previewState.status !== "success") {
      return null;
    }

    return (
      previewState.response.executables.find(
        (candidate) => candidate.path === selectedExecutablePath,
      ) ?? null
    );
  }, [previewState, selectedExecutablePath]);

  const canInstall =
    Boolean(installTarget && selectedExecutable) && installState.status !== "running";
  const installedApps = installedAppsState.status === "success" ? installedAppsState.apps : [];
  const selectedUninstallApp =
    installedApps.find((app) => app.appName === selectedUninstallAppName) ?? null;
  const selectedUninstallOptionValue = selectedUninstallApp?.appName ?? "";
  const canUninstall = Boolean(selectedUninstallApp) && uninstallState.status !== "running";

  useEffect(() => {
    if (!installTarget) {
      setPreviewState({ status: "idle" });
      setSelectedExecutablePath("");
      setDesktopName("");
      setDesktopIconPath("");
      setDesktopCategories("Utility;");
      setDesktopTerminal(false);
      return;
    }

    let isCurrent = true;

    async function previewSelectedPackage(target: InstallTarget) {
      setPreviewState({ status: "running" });
      setSelectedExecutablePath("");
      setDesktopName("");
      setDesktopIconPath("");
      setDesktopCategories("Utility;");
      setDesktopTerminal(false);

      try {
        const response = await invoke<PreviewResponse>("preview_package", {
          request: {
            sourcePath: target.sourcePath,
            packageKind: target.kind,
          },
        });

        if (!isCurrent) {
          return;
        }

        setPreviewState({ status: "success", response });
        setSelectedExecutablePath(response.executables[0]?.path ?? "");
        setDesktopName(response.desktopSuggestion.name);
        setDesktopIconPath(response.desktopSuggestion.iconPath ?? "");
        setDesktopCategories(response.desktopSuggestion.categories);
        setDesktopTerminal(response.desktopSuggestion.terminal);
      } catch (error) {
        if (!isCurrent) {
          return;
        }

        setPreviewState({
          status: "error",
          message: error instanceof Error ? error.message : String(error),
        });
      }
    }

    previewSelectedPackage(installTarget);

    return () => {
      isCurrent = false;
    };
  }, [installTarget]);

  useEffect(() => {
    let isCurrent = true;

    async function loadInstalledApps() {
      setInstalledAppsState({ status: "running" });

      try {
        const apps = await invoke<InstalledApp[]>("list_installed_apps", {
          request: {
            installDir: installPath,
          },
        });

        if (!isCurrent) {
          return;
        }

        setInstalledAppsState({ status: "success", apps });
        setSelectedUninstallAppName((current) =>
          apps.some((app) => app.appName === current) ? current : apps[0]?.appName ?? "",
        );
      } catch (error) {
        if (!isCurrent) {
          return;
        }

        setInstalledAppsState({
          status: "error",
          message: error instanceof Error ? error.message : String(error),
        });
        setSelectedUninstallAppName("");
      }
    }

    loadInstalledApps();

    return () => {
      isCurrent = false;
    };
  }, [installPath]);

  async function refreshInstalledApps() {
    try {
      const apps = await invoke<InstalledApp[]>("list_installed_apps", {
        request: {
          installDir: installPath,
        },
      });

      setInstalledAppsState({ status: "success", apps });
      setSelectedUninstallAppName((current) =>
        apps.some((app) => app.appName === current) ? current : apps[0]?.appName ?? "",
      );
    } catch (error) {
      setInstalledAppsState({
        status: "error",
        message: error instanceof Error ? error.message : String(error),
      });
      setSelectedUninstallAppName("");
    }
  }

  async function handlePickFile() {
    try {
      const selectedPath = await open({
        multiple: false,
        filters: packageDialogFilters,
      });

      if (typeof selectedPath !== "string") {
        return;
      }

      const sourceInfo = await invoke<SourceFileInfo>("inspect_source_file", {
        sourcePath: selectedPath,
      });

      setInstallTarget({
        fileName: sourceInfo.fileName,
        fileSize: sourceInfo.fileSize,
        kind: detectPackageKind(sourceInfo.fileName),
        sourcePath: sourceInfo.sourcePath,
      });
      setInstallState({ status: "idle" });
    } catch (error) {
      setInstallTarget(null);
      setPreviewState({
        status: "error",
        message: error instanceof Error ? error.message : String(error),
      });
      setInstallState({ status: "idle" });
    }
  }

  async function handleInstall() {
    if (!installTarget || !selectedExecutable || installState.status === "running") {
      return;
    }

    setInstallState({ status: "running" });

    try {
      const response = await invoke<InstallResponse>("install_package", {
        request: {
          sourcePath: installTarget.sourcePath,
          installDir: installPath,
          packageKind: installTarget.kind,
          previewRoot:
            previewState.status === "success" ? previewState.response.previewRoot : null,
          selectedExecutablePath,
          desktopConfig: {
            name: desktopName,
            iconPath: desktopIconPath || null,
            terminal: desktopTerminal,
            categories: desktopCategories,
          },
          createDesktopEntry,
          addToPath,
        },
      });

      setInstallState({ status: "success", response });
      await refreshInstalledApps();
    } catch (error) {
      setInstallState({
        status: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }

  async function handleUninstall() {
    if (!selectedUninstallApp || uninstallState.status === "running") {
      return;
    }

    const confirmed = window.confirm(`确认卸载 ${selectedUninstallApp.appName}？`);
    if (!confirmed) {
      return;
    }

    setUninstallState({ status: "running" });

    try {
      const response = await invoke<UninstallResponse>("uninstall_app", {
        request: {
          installDir: installPath,
          appName: selectedUninstallApp.appName,
        },
      });

      setUninstallState({ status: "success", response });
      await refreshInstalledApps();
    } catch (error) {
      setUninstallState({
        status: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }

  return (
    <main className="app-shell">
      <section className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">Arch Auto Installer</p>
            <h1>快捷安装器</h1>
          </div>
          <div className="status-pill">本地安装</div>
        </header>

        <section className="installer-grid">
          <div className="install-source">
            <div className="section-heading">
              <span>01</span>
              <h2>安装文件</h2>
            </div>

            <button className="file-picker" onClick={handlePickFile} type="button">
              <span className="file-picker-icon" aria-hidden="true">
                +
              </span>
              <span>
                <strong>选择安装文件</strong>
                <small>{supportedTypes.join(" / ")}</small>
              </span>
            </button>

            <div className="file-summary">
              <div>
                <span className="summary-label">文件名</span>
                <strong>{installTarget?.fileName ?? "未选择"}</strong>
              </div>
              <div>
                <span className="summary-label">类型</span>
                <strong>{installTarget?.kind ?? "待检测"}</strong>
              </div>
              <div>
                <span className="summary-label">大小</span>
                <strong>
                  {installTarget ? formatFileSize(installTarget.fileSize) : "-"}
                </strong>
              </div>
              <div>
                <span className="summary-label">路径</span>
                <strong>{installTarget?.sourcePath ?? "-"}</strong>
              </div>
            </div>

            <div className={`preview-panel preview-${previewState.status}`}>
              <div className="preview-header">
                <span className="summary-label">导入检查</span>
                <strong>
                  {previewState.status === "idle" && "等待导入"}
                  {previewState.status === "running" && "正在临时解压并查找入口"}
                  {previewState.status === "success" &&
                    `找到 ${previewState.response.executables.length} 个可执行文件`}
                  {previewState.status === "error" && "检查失败"}
                </strong>
              </div>

              {previewState.status === "error" && (
                <p className="preview-message">{previewState.message}</p>
              )}

              {previewState.status === "success" && (
                <>
                  <div className="preview-root">
                    <span className="summary-label">临时目录</span>
                    <strong>{previewState.response.previewRoot}</strong>
                  </div>

                  <div className="desktop-preview">
                    <div>
                      <span className="summary-label">桌面名称</span>
                      <strong>{previewState.response.desktopSuggestion.name}</strong>
                    </div>
                    <div>
                      <span className="summary-label">图标</span>
                      <strong>
                        {previewState.response.desktopSuggestion.iconRelativePath ??
                          "未检测到"}
                      </strong>
                    </div>
                  </div>

                  <div className="candidate-list" role="radiogroup" aria-label="可执行文件">
                    {previewState.response.executables.map((candidate) => (
                      <label className="candidate-row" key={candidate.path}>
                        <input
                          checked={selectedExecutablePath === candidate.path}
                          name="executable"
                          onChange={() => setSelectedExecutablePath(candidate.path)}
                          type="radio"
                        />
                        <span>
                          <strong>{candidate.fileName}</strong>
                          <small>{candidate.relativePath}</small>
                        </span>
                      </label>
                    ))}
                  </div>
                </>
              )}
            </div>
          </div>

          <aside className="install-options">
            <div className="section-heading">
              <span>02</span>
              <h2>安装选项</h2>
            </div>

            <label className="field">
              <span>安装目录</span>
              <input
                onChange={(event) => setInstallPath(event.currentTarget.value)}
                value={installPath}
              />
            </label>

            <label className="toggle-row">
              <input
                checked={createDesktopEntry}
                onChange={(event) => setCreateDesktopEntry(event.currentTarget.checked)}
                type="checkbox"
              />
              <span>
                <strong>创建桌面入口</strong>
                <small>生成 .desktop 文件</small>
              </span>
            </label>

            {createDesktopEntry && (
              <div className="desktop-options">
                <label className="field">
                  <span>应用名称</span>
                  <input
                    onChange={(event) => setDesktopName(event.currentTarget.value)}
                    value={desktopName}
                  />
                </label>

                <label className="field">
                  <span>图标</span>
                  <select
                    disabled={previewState.status !== "success"}
                    onChange={(event) => setDesktopIconPath(event.currentTarget.value)}
                    value={desktopIconPath}
                  >
                    <option value="">不使用图标</option>
                    {previewState.status === "success" &&
                      previewState.response.iconCandidates.map((icon) => (
                        <option key={icon.path} value={icon.path}>
                          {icon.relativePath}
                        </option>
                      ))}
                  </select>
                </label>

                <label className="field">
                  <span>分类</span>
                  <input
                    onChange={(event) => setDesktopCategories(event.currentTarget.value)}
                    value={desktopCategories}
                  />
                </label>

                <label className="toggle-row compact-toggle">
                  <input
                    checked={desktopTerminal}
                    onChange={(event) => setDesktopTerminal(event.currentTarget.checked)}
                    type="checkbox"
                  />
                  <span>
                    <strong>终端运行</strong>
                    <small>写入 Terminal=true</small>
                  </span>
                </label>
              </div>
            )}

            <label className="toggle-row">
              <input
                checked={addToPath}
                onChange={(event) => setAddToPath(event.currentTarget.checked)}
                type="checkbox"
              />
              <span>
                <strong>加入 PATH</strong>
                <small>适合命令行工具</small>
              </span>
            </label>
          </aside>
        </section>

        <section className="queue-panel">
          <div className="section-heading">
            <span>03</span>
            <h2>安装计划</h2>
          </div>

          <div className="plan-list">
            {installPlan.map((step, index) => (
              <div className="plan-item" key={step}>
                <span>{index + 1}</span>
                <p>{step}</p>
              </div>
            ))}
          </div>

            <div className="action-bar">
              <div>
                <span className="summary-label">目标位置</span>
                <strong>{installPath || "未设置"}</strong>
              </div>
              <div>
                <span className="summary-label">已确认入口</span>
                <strong>{selectedExecutable?.relativePath ?? "未确认"}</strong>
              </div>
            <button
              disabled={!canInstall}
              onClick={handleInstall}
              type="button"
            >
              {installState.status === "running" ? "安装中" : "开始安装"}
            </button>
          </div>

          {installState.status !== "idle" && (
            <section className={`result-panel result-${installState.status}`}>
              <div className="section-heading">
                <span>04</span>
                <h2>安装结果</h2>
              </div>

              {installState.status === "running" && (
                <p className="result-message">正在处理安装包...</p>
              )}

              {installState.status === "error" && (
                <p className="result-message">{installState.message}</p>
              )}

              {installState.status === "success" && (
                <>
                  <div className="result-summary">
                    <div>
                      <span className="summary-label">应用</span>
                      <strong>{installState.response.appName}</strong>
                    </div>
                    <div>
                      <span className="summary-label">安装位置</span>
                      <strong>{installState.response.installRoot}</strong>
                    </div>
                    <div>
                      <span className="summary-label">入口</span>
                      <strong>{installState.response.executablePath ?? "-"}</strong>
                    </div>
                  </div>

                  <div className="step-log">
                    {installState.response.steps.map((step) => (
                      <div className={`step-row step-${step.status}`} key={`${step.title}-${step.detail}`}>
                        <span>{step.status === "done" ? "完成" : "跳过"}</span>
                        <div>
                          <strong>{step.title}</strong>
                          <p>{step.detail}</p>
                        </div>
                      </div>
                    ))}
                  </div>
                </>
              )}
            </section>
          )}

          <section className="uninstall-panel">
            <div className="section-heading">
              <span>05</span>
              <h2>卸载应用</h2>
            </div>

            <div className="uninstall-layout">
              <label className="field">
                <span>已安装应用</span>
                <select
                  disabled={installedAppsState.status !== "success" || installedApps.length === 0}
                  onChange={(event) => {
                    setSelectedUninstallAppName(event.currentTarget.value);
                    setUninstallState({ status: "idle" });
                  }}
                  value={selectedUninstallOptionValue}
                >
                  {installedApps.length === 0 && <option value="">未检测到应用</option>}
                  {installedApps.map((app) => (
                    <option key={app.installRoot} value={app.appName}>
                      {app.appName}
                    </option>
                  ))}
                </select>
              </label>

              <div>
                <span className="summary-label">安装位置</span>
                <strong>{selectedUninstallApp?.installRoot ?? "-"}</strong>
              </div>

              <button disabled={!canUninstall} onClick={handleUninstall} type="button">
                {uninstallState.status === "running" ? "卸载中" : "卸载"}
              </button>
            </div>

            {installedAppsState.status === "running" && (
              <p className="result-message">正在读取已安装应用...</p>
            )}

            {installedAppsState.status === "error" && (
              <p className="result-message">{installedAppsState.message}</p>
            )}

            {selectedUninstallApp && (
              <div className="uninstall-details">
                <div>
                  <span className="summary-label">入口</span>
                  <strong>{selectedUninstallApp.executablePath ?? "-"}</strong>
                </div>
                <div>
                  <span className="summary-label">桌面入口</span>
                  <strong>{selectedUninstallApp.desktopEntryPath ?? "-"}</strong>
                </div>
                <div>
                  <span className="summary-label">PATH 链接</span>
                  <strong>{selectedUninstallApp.pathLink ?? "-"}</strong>
                </div>
              </div>
            )}

            {uninstallState.status === "error" && (
              <p className="result-message result-message-error">{uninstallState.message}</p>
            )}

            {uninstallState.status === "success" && (
              <div className="step-log">
                {uninstallState.response.steps.map((step) => (
                  <div
                    className={`step-row step-${step.status}`}
                    key={`${step.title}-${step.detail}`}
                  >
                    <span>{step.status === "done" ? "完成" : "跳过"}</span>
                    <div>
                      <strong>{step.title}</strong>
                      <p>{step.detail}</p>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </section>
      </section>
    </main>
  );
}

export default App;
