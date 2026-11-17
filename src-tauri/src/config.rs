use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
        pub id: String,
        pub name: String,
        pub prompt: String,
        #[serde(default)]
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerConfig {
        pub mode: String,
            #[serde(default, skip_serializing)]
    pub hotkey: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HotkeysConfig {
        pub show_window: String,
        pub translate: String,
        pub disable: String,
        pub enable: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateConfig {
        pub enabled: bool,
        pub mode: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "notify".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiConfig {
        pub provider: String,
        pub base_url: String,
        pub model: String,
        pub api_key: String,
        pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageConfig {
        pub source: String,
        pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
        #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    #[serde(default)]
    pub maximized: bool,
}

pub const MIN_WINDOW_WIDTH: u32 = 1024;
pub const MIN_WINDOW_HEIGHT: u32 = 700;

impl Default for WindowState {
        fn default() -> Self {
        Self {
            width: MIN_WINDOW_WIDTH,
            height: MIN_WINDOW_HEIGHT,
            x: None,
            y: None,
            maximized: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub version: u32,
        pub theme: String,
    pub trigger: TriggerConfig,
        #[serde(default)]
    pub hotkeys: HotkeysConfig,
        #[serde(default)]
    pub update: UpdateConfig,
    pub api: ApiConfig,
    pub scenes: Vec<Scene>,
        pub active_scene_id: String,
    pub languages: LanguageConfig,
        pub stream: bool,
        #[serde(default = "default_true")]
    pub selection_enabled: bool,
            #[serde(
        default = "default_selection_threshold",
        alias = "maxSelectionLength"
    )]
    pub selection_threshold: u32,
        #[serde(default)]
    pub window: WindowState,
}

fn default_selection_threshold() -> u32 {
    5000
}

fn default_true() -> bool {
    true
}

pub const DEFAULT_GENERAL_PROMPT: &str = "你是一个专业的翻译引擎。请把用户提供的文本翻译成{target}。\n\
要求：\n\
1. 只输出译文，不要解释、不要加引号、不要复述原文。\n\
2. 保持原文的段落结构和换行。\n\
3. 保留原文中的专有名词、代码、变量名、URL 不变。\n\
4. 如果原文本身已是{target}，则原样返回。";

pub const DEFAULT_TECH_PROMPT: &str = "你是一名资深软件技术文档译者。请把用户提供的技术文本翻译成{target}。\n\
要求：\n\
1. 术语准确：API、函数名、类名、变量名、命令行、代码片段、文件路径一律保留英文原文。\n\
2. 语气保持技术文档的简洁与客观，不添加额外说明。\n\
3. 保留 Markdown 标记、列表层级、代码块与换行结构。\n\
4. 只输出译文。";

pub const DEFAULT_ACADEMIC_PROMPT: &str = "你是一名学术论文译者。请将用户提供的学术文本翻译成{target}。\n\
要求：\n\
1. 使用规范的学术书面语，术语前后一致。\n\
2. 公式、符号、引用编号、图表编号保持原样。\n\
3. 长句按中文/目标语习惯拆分，但不得改变原意与逻辑关系。\n\
4. 只输出译文。";

pub const DEFAULT_CASUAL_PROMPT: &str = "你是一个母语级的日常口语译者。请把用户提供的内容翻译成{target}。\n\
要求：\n\
1. 译文要自然、口语化，符合目标语言的日常表达习惯，不要生硬直译。\n\
2. 网络用语、语气词尽量用目标语言中等价的表达。\n\
3. 只输出译文。";

pub const DEFAULT_EXPLAIN_PROMPT: &str = "你是一名双语讲解老师。用户会给你一段文本，请用{target}输出：\n\
1. 先给出这段文本自然流畅的完整译文；\n\
2. 再用「要点」形式简要说明其中的关键词、难点表达或文化背景（不超过 5 条）。\n\
输出使用 Markdown。";

fn builtin_scenes() -> Vec<Scene> {
    vec![
        Scene {
            id: "general".into(),
            name: "通用翻译".into(),
            prompt: DEFAULT_GENERAL_PROMPT.into(),
            builtin: true,
        },
        Scene {
            id: "tech".into(),
            name: "技术文档".into(),
            prompt: DEFAULT_TECH_PROMPT.into(),
            builtin: true,
        },
        Scene {
            id: "academic".into(),
            name: "学术论文".into(),
            prompt: DEFAULT_ACADEMIC_PROMPT.into(),
            builtin: true,
        },
        Scene {
            id: "casual".into(),
            name: "口语日常".into(),
            prompt: DEFAULT_CASUAL_PROMPT.into(),
            builtin: true,
        },
        Scene {
            id: "explain".into(),
            name: "译文 + 讲解".into(),
            prompt: DEFAULT_EXPLAIN_PROMPT.into(),
            builtin: true,
        },
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            theme: "light".into(),
            trigger: TriggerConfig {
                mode: "button".into(),
                hotkey: String::new(),
            },
            hotkeys: HotkeysConfig {
                show_window: String::new(),
                translate: "Alt+Shift+KeyT".into(),
                disable: String::new(),
                enable: String::new(),
            },
            update: UpdateConfig::default(),
            api: ApiConfig {
                provider: "deepseek".into(),
                base_url: "https://api.deepseek.com".into(),
                model: "deepseek-chat".into(),
                api_key: String::new(),
                temperature: 0.3,
            },
            scenes: builtin_scenes(),
            active_scene_id: "general".into(),
            languages: LanguageConfig {
                source: "auto".into(),
                target: "简体中文".into(),
            },
            stream: true,
            selection_enabled: true,
            selection_threshold: default_selection_threshold(),
            window: WindowState::default(),
        }
    }
}

pub fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法定位配置目录: {e}"))?;
    Ok(dir.join(CONFIG_FILE))
}

pub fn load_or_init(app: &AppHandle) -> Config {
    let path = match config_path(app) {
        Ok(p) => p,
        Err(_) => return Config::default(),
    };

    if let Ok(text) = std::fs::read_to_string(&path) {
        match serde_json::from_str::<Config>(&text) {
            Ok(mut cfg) => {
                                merge_builtin_scenes(&mut cfg);
                                if migrate_legacy(&mut cfg) {
                    let _ = save(app, &cfg);
                }
                return cfg;
            }
            Err(_) => {
                                let bak = path.with_extension("json.bak");
                let _ = std::fs::copy(&path, bak);
            }
        }
    }

    let cfg = Config::default();
    let _ = save(app, &cfg);
    cfg
}

fn migrate_legacy(cfg: &mut Config) -> bool {
    let mut changed = false;

    if cfg.trigger.mode == "selection" {
        cfg.trigger.mode = "button".into();
        changed = true;
    }

    if cfg.hotkeys.translate.trim().is_empty() && !cfg.trigger.hotkey.trim().is_empty() {
        cfg.hotkeys.translate = cfg.trigger.hotkey.clone();
        changed = true;
    }

    if !cfg.trigger.hotkey.is_empty() {
        cfg.trigger.hotkey.clear();
        changed = true;
    }

    changed
}

fn merge_builtin_scenes(cfg: &mut Config) {
    for builtin in builtin_scenes() {
        if !cfg.scenes.iter().any(|s| s.id == builtin.id) {
            cfg.scenes.push(builtin);
        } else {
                        if let Some(existing) = cfg.scenes.iter_mut().find(|s| s.id == builtin.id) {
                existing.builtin = true;
            }
        }
    }
}

pub fn save(app: &AppHandle, cfg: &Config) -> Result<PathBuf, String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("写入配置失败: {e}"))?;
    Ok(path)
}

pub fn default_prompt_for(id: &str) -> Option<&'static str> {
    match id {
        "general" => Some(DEFAULT_GENERAL_PROMPT),
        "tech" => Some(DEFAULT_TECH_PROMPT),
        "academic" => Some(DEFAULT_ACADEMIC_PROMPT),
        "casual" => Some(DEFAULT_CASUAL_PROMPT),
        "explain" => Some(DEFAULT_EXPLAIN_PROMPT),
        _ => None,
    }
}
