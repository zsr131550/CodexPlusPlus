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
}

impl TextKey {
    pub const ALL: [Self; 42] = [
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
    ];
}

struct CatalogEntry {
    zh: &'static str,
    en: &'static str,
}

const CATALOG: [CatalogEntry; 42] = [
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
    fn every_milestone_one_key_has_both_locales() {
        assert_eq!(TextKey::ALL.len(), 42);
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
