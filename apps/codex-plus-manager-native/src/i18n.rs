#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Locale {
    #[default]
    ZhCn,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKey {
    AppName,
    Overview,
    Providers,
    About,
    OverviewSubtitle,
    ProvidersSubtitle,
    AboutSubtitle,
    Refresh,
    Refreshing,
    Ready,
    Loading,
    Retry,
    CodexApplication,
    SilentEntrypoint,
    ManagementEntrypoint,
    Found,
    Missing,
    Installed,
    LatestLaunch,
    NoLaunch,
    Status,
    StartedAt,
    DebugPort,
    HelperPort,
    CodexPath,
    Version,
    CodexPlusVersion,
    CodexVersion,
    LocalPaths,
    SettingsPath,
    LogsPath,
    Language,
    Theme,
    Chinese,
    English,
    Dark,
    Light,
    LoadFailed,
    WorkerStopped,
    Renderer,
    License,
    LastUpdated,
    Environment,
    EnvironmentSubtitle,
    ImportProviders,
    ImportFromCcs,
    ImportSource,
    Importable,
    Duplicates,
    Duplicate,
    ImportNew,
    Cancel,
    PendingImport,
    ConfirmImport,
    DismissImport,
    RefreshPendingImport,
    ApiKeyPresent,
    ApiKeyMissing,
    ProviderDraftDirty,
    ImportSucceeded,
    NoImportableProviders,
    RelayDiagnostics,
    TunMode,
    ProxyEnvironment,
    CodexEnvFile,
    OpenAiConflicts,
    Enabled,
    Disabled,
    Present,
    NotPresent,
    Process,
    User,
    System,
    Removed,
    Failed,
    CleanupSelected,
    ConfirmCleanup,
    CleanupSucceeded,
    CleanupPartial,
    BackupCreated,
    NoBackup,
    RemainingConflicts,
    NoConflicts,
    InspectionFailed,
    CleanupFailed,
    SourceChanged,
    EnvironmentChanged,
    PendingChanged,
    ProviderChanged,
    WireApi,
    RelayMode,
    BaseUrl,
    ImportedCount,
    FailureCount,
    SelectAtLeastOne,
    InProgress,
    ReviewPendingImport,
    EnvironmentHealthy,
    EnvironmentIssues,
    CleanupConfirmationTitle,
    CcsImportTitle,
    PendingImportTitle,
    RefreshEnvironment,
    EnvironmentWorkerStopped,
    ImportWorkerStopped,
    RetryInspection,
}

impl TextKey {
    pub const ALL: [Self; 106] = [
        Self::AppName,
        Self::Overview,
        Self::Providers,
        Self::About,
        Self::OverviewSubtitle,
        Self::ProvidersSubtitle,
        Self::AboutSubtitle,
        Self::Refresh,
        Self::Refreshing,
        Self::Ready,
        Self::Loading,
        Self::Retry,
        Self::CodexApplication,
        Self::SilentEntrypoint,
        Self::ManagementEntrypoint,
        Self::Found,
        Self::Missing,
        Self::Installed,
        Self::LatestLaunch,
        Self::NoLaunch,
        Self::Status,
        Self::StartedAt,
        Self::DebugPort,
        Self::HelperPort,
        Self::CodexPath,
        Self::Version,
        Self::CodexPlusVersion,
        Self::CodexVersion,
        Self::LocalPaths,
        Self::SettingsPath,
        Self::LogsPath,
        Self::Language,
        Self::Theme,
        Self::Chinese,
        Self::English,
        Self::Dark,
        Self::Light,
        Self::LoadFailed,
        Self::WorkerStopped,
        Self::Renderer,
        Self::License,
        Self::LastUpdated,
        Self::Environment,
        Self::EnvironmentSubtitle,
        Self::ImportProviders,
        Self::ImportFromCcs,
        Self::ImportSource,
        Self::Importable,
        Self::Duplicates,
        Self::Duplicate,
        Self::ImportNew,
        Self::Cancel,
        Self::PendingImport,
        Self::ConfirmImport,
        Self::DismissImport,
        Self::RefreshPendingImport,
        Self::ApiKeyPresent,
        Self::ApiKeyMissing,
        Self::ProviderDraftDirty,
        Self::ImportSucceeded,
        Self::NoImportableProviders,
        Self::RelayDiagnostics,
        Self::TunMode,
        Self::ProxyEnvironment,
        Self::CodexEnvFile,
        Self::OpenAiConflicts,
        Self::Enabled,
        Self::Disabled,
        Self::Present,
        Self::NotPresent,
        Self::Process,
        Self::User,
        Self::System,
        Self::Removed,
        Self::Failed,
        Self::CleanupSelected,
        Self::ConfirmCleanup,
        Self::CleanupSucceeded,
        Self::CleanupPartial,
        Self::BackupCreated,
        Self::NoBackup,
        Self::RemainingConflicts,
        Self::NoConflicts,
        Self::InspectionFailed,
        Self::CleanupFailed,
        Self::SourceChanged,
        Self::EnvironmentChanged,
        Self::PendingChanged,
        Self::ProviderChanged,
        Self::WireApi,
        Self::RelayMode,
        Self::BaseUrl,
        Self::ImportedCount,
        Self::FailureCount,
        Self::SelectAtLeastOne,
        Self::InProgress,
        Self::ReviewPendingImport,
        Self::EnvironmentHealthy,
        Self::EnvironmentIssues,
        Self::CleanupConfirmationTitle,
        Self::CcsImportTitle,
        Self::PendingImportTitle,
        Self::RefreshEnvironment,
        Self::EnvironmentWorkerStopped,
        Self::ImportWorkerStopped,
        Self::RetryInspection,
    ];
}

struct CatalogEntry {
    zh: &'static str,
    en: &'static str,
}

const CATALOG: [CatalogEntry; 106] = [
    CatalogEntry {
        zh: "Codex++",
        en: "Codex++",
    },
    CatalogEntry {
        zh: "概览",
        en: "Overview",
    },
    CatalogEntry {
        zh: "供应商配置",
        en: "Providers",
    },
    CatalogEntry {
        zh: "关于",
        en: "About",
    },
    CatalogEntry {
        zh: "检查本地安装与运行状态",
        en: "Inspect local installation and runtime",
    },
    CatalogEntry {
        zh: "管理供应商、模型与聚合路由",
        en: "Manage providers, models, and aggregate routing",
    },
    CatalogEntry {
        zh: "原生管理工具信息",
        en: "Native manager information",
    },
    CatalogEntry {
        zh: "刷新",
        en: "Refresh",
    },
    CatalogEntry {
        zh: "正在刷新",
        en: "Refreshing",
    },
    CatalogEntry {
        zh: "已就绪",
        en: "Ready",
    },
    CatalogEntry {
        zh: "正在加载",
        en: "Loading",
    },
    CatalogEntry {
        zh: "重试",
        en: "Retry",
    },
    CatalogEntry {
        zh: "Codex 应用",
        en: "Codex application",
    },
    CatalogEntry {
        zh: "静默启动入口",
        en: "Launcher entry point",
    },
    CatalogEntry {
        zh: "管理工具入口",
        en: "Manager entry point",
    },
    CatalogEntry {
        zh: "已找到",
        en: "Found",
    },
    CatalogEntry {
        zh: "未找到",
        en: "Missing",
    },
    CatalogEntry {
        zh: "已安装",
        en: "Installed",
    },
    CatalogEntry {
        zh: "最近启动",
        en: "Latest launch",
    },
    CatalogEntry {
        zh: "暂无启动记录",
        en: "No launch recorded",
    },
    CatalogEntry {
        zh: "状态",
        en: "Status",
    },
    CatalogEntry {
        zh: "启动时间",
        en: "Started at",
    },
    CatalogEntry {
        zh: "调试端口",
        en: "Debug port",
    },
    CatalogEntry {
        zh: "辅助端口",
        en: "Helper port",
    },
    CatalogEntry {
        zh: "Codex 路径",
        en: "Codex path",
    },
    CatalogEntry {
        zh: "版本",
        en: "Version",
    },
    CatalogEntry {
        zh: "Codex++ 版本",
        en: "Codex++ version",
    },
    CatalogEntry {
        zh: "Codex 版本",
        en: "Codex version",
    },
    CatalogEntry {
        zh: "本地路径",
        en: "Local paths",
    },
    CatalogEntry {
        zh: "设置文件",
        en: "Settings file",
    },
    CatalogEntry {
        zh: "诊断日志",
        en: "Diagnostic log",
    },
    CatalogEntry {
        zh: "语言",
        en: "Language",
    },
    CatalogEntry {
        zh: "主题",
        en: "Theme",
    },
    CatalogEntry {
        zh: "中文",
        en: "Chinese",
    },
    CatalogEntry {
        zh: "English",
        en: "English",
    },
    CatalogEntry {
        zh: "深色",
        en: "Dark",
    },
    CatalogEntry {
        zh: "浅色",
        en: "Light",
    },
    CatalogEntry {
        zh: "无法加载概览。",
        en: "Unable to load the overview.",
    },
    CatalogEntry {
        zh: "后台服务已停止。",
        en: "The background service has stopped.",
    },
    CatalogEntry {
        zh: "渲染器",
        en: "Renderer",
    },
    CatalogEntry {
        zh: "许可证",
        en: "License",
    },
    CatalogEntry {
        zh: "最近更新",
        en: "Last updated",
    },
    CatalogEntry {
        zh: "环境检查",
        en: "Environment",
    },
    CatalogEntry {
        zh: "检查影响供应商连接的本机环境",
        en: "Inspect the local environment affecting provider connections",
    },
    CatalogEntry {
        zh: "导入供应商",
        en: "Import providers",
    },
    CatalogEntry {
        zh: "从 cc-switch 导入",
        en: "Import from cc-switch",
    },
    CatalogEntry {
        zh: "导入来源",
        en: "Import source",
    },
    CatalogEntry {
        zh: "可导入",
        en: "Importable",
    },
    CatalogEntry {
        zh: "重复项",
        en: "Duplicates",
    },
    CatalogEntry {
        zh: "已存在",
        en: "Duplicate",
    },
    CatalogEntry {
        zh: "导入新增项",
        en: "Import new",
    },
    CatalogEntry {
        zh: "取消",
        en: "Cancel",
    },
    CatalogEntry {
        zh: "待处理导入",
        en: "Pending import",
    },
    CatalogEntry {
        zh: "确认导入",
        en: "Confirm import",
    },
    CatalogEntry {
        zh: "放弃导入",
        en: "Dismiss import",
    },
    CatalogEntry {
        zh: "刷新待确认导入",
        en: "Refresh pending import",
    },
    CatalogEntry {
        zh: "已提供 API 密钥",
        en: "API key provided",
    },
    CatalogEntry {
        zh: "未提供 API 密钥",
        en: "No API key provided",
    },
    CatalogEntry {
        zh: "请先保存或放弃供应商草稿。",
        en: "Save or discard the provider draft first.",
    },
    CatalogEntry {
        zh: "供应商导入完成",
        en: "Provider import completed",
    },
    CatalogEntry {
        zh: "没有新的供应商可导入",
        en: "No new providers to import",
    },
    CatalogEntry {
        zh: "中转环境诊断",
        en: "Relay environment diagnostics",
    },
    CatalogEntry {
        zh: "TUN 模式",
        en: "TUN mode",
    },
    CatalogEntry {
        zh: "代理环境变量",
        en: "Proxy environment",
    },
    CatalogEntry {
        zh: "Codex .env 文件",
        en: "Codex .env file",
    },
    CatalogEntry {
        zh: "OPENAI 环境冲突",
        en: "OPENAI environment conflicts",
    },
    CatalogEntry {
        zh: "已启用",
        en: "Enabled",
    },
    CatalogEntry {
        zh: "未启用",
        en: "Disabled",
    },
    CatalogEntry {
        zh: "存在",
        en: "Present",
    },
    CatalogEntry {
        zh: "不存在",
        en: "Not present",
    },
    CatalogEntry {
        zh: "进程",
        en: "Process",
    },
    CatalogEntry {
        zh: "用户",
        en: "User",
    },
    CatalogEntry {
        zh: "系统",
        en: "System",
    },
    CatalogEntry {
        zh: "已删除",
        en: "Removed",
    },
    CatalogEntry {
        zh: "失败",
        en: "Failed",
    },
    CatalogEntry {
        zh: "清理所选项",
        en: "Clean selected",
    },
    CatalogEntry {
        zh: "确认清理",
        en: "Confirm cleanup",
    },
    CatalogEntry {
        zh: "环境冲突清理完成",
        en: "Environment cleanup completed",
    },
    CatalogEntry {
        zh: "部分环境冲突未能清理",
        en: "Some environment conflicts could not be removed",
    },
    CatalogEntry {
        zh: "已创建元数据备份",
        en: "Metadata backup created",
    },
    CatalogEntry {
        zh: "无需备份",
        en: "No backup required",
    },
    CatalogEntry {
        zh: "剩余冲突",
        en: "Remaining conflicts",
    },
    CatalogEntry {
        zh: "未检测到冲突",
        en: "No conflicts detected",
    },
    CatalogEntry {
        zh: "环境检查失败",
        en: "Environment inspection failed",
    },
    CatalogEntry {
        zh: "环境清理失败",
        en: "Environment cleanup failed",
    },
    CatalogEntry {
        zh: "导入来源已变化，请重新检查。",
        en: "The import source changed. Inspect it again.",
    },
    CatalogEntry {
        zh: "环境状态已变化，请重新检查。",
        en: "The environment changed. Inspect it again.",
    },
    CatalogEntry {
        zh: "待处理导入已变化，请重新读取。",
        en: "The pending import changed. Load it again.",
    },
    CatalogEntry {
        zh: "供应商配置已变化，请重新加载。",
        en: "Provider settings changed. Reload them.",
    },
    CatalogEntry {
        zh: "接口协议",
        en: "Wire API",
    },
    CatalogEntry {
        zh: "中转模式",
        en: "Relay mode",
    },
    CatalogEntry {
        zh: "基础地址",
        en: "Base URL",
    },
    CatalogEntry {
        zh: "已导入数量",
        en: "Imported count",
    },
    CatalogEntry {
        zh: "失败数量",
        en: "Failure count",
    },
    CatalogEntry {
        zh: "请至少选择一个冲突项。",
        en: "Select at least one conflict.",
    },
    CatalogEntry {
        zh: "正在处理",
        en: "In progress",
    },
    CatalogEntry {
        zh: "查看待处理导入",
        en: "Review pending import",
    },
    CatalogEntry {
        zh: "环境检查通过",
        en: "Environment checks passed",
    },
    CatalogEntry {
        zh: "检测到环境问题",
        en: "Environment issues detected",
    },
    CatalogEntry {
        zh: "确认清理环境变量",
        en: "Confirm environment cleanup",
    },
    CatalogEntry {
        zh: "cc-switch 供应商导入",
        en: "cc-switch provider import",
    },
    CatalogEntry {
        zh: "确认待处理供应商导入",
        en: "Confirm pending provider import",
    },
    CatalogEntry {
        zh: "刷新环境检查",
        en: "Refresh environment checks",
    },
    CatalogEntry {
        zh: "环境检查后台服务已停止。",
        en: "The environment worker has stopped.",
    },
    CatalogEntry {
        zh: "供应商导入后台服务已停止。",
        en: "The provider import worker has stopped.",
    },
    CatalogEntry {
        zh: "重新检查",
        en: "Inspect again",
    },
];

pub fn text(locale: Locale, key: TextKey) -> &'static str {
    let entry = &CATALOG[key as usize];
    match locale {
        Locale::ZhCn => entry.zh,
        Locale::En => entry.en,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_native_manager_key_has_both_locales() {
        assert_eq!(TextKey::ALL.len(), CATALOG.len());
        for key in TextKey::ALL {
            assert!(
                !text(Locale::ZhCn, key).trim().is_empty(),
                "missing zh: {key:?}"
            );
            assert!(
                !text(Locale::En, key).trim().is_empty(),
                "missing en: {key:?}"
            );
        }
    }

    #[test]
    fn locale_defaults_to_chinese_and_switches_catalog_values() {
        assert_eq!(Locale::default(), Locale::ZhCn);
        assert_eq!(text(Locale::ZhCn, TextKey::About), "关于");
        assert_eq!(text(Locale::En, TextKey::About), "About");
    }
}
