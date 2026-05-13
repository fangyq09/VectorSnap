#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
//use pdfium_render::prelude::*;
use pdfium_render::prelude::{Pdfium, PdfDocument, PdfPage, PdfColor,
PdfPoints, PdfPageRenderRotation, PdfRenderConfig, 
PdfPageObjectsCommon, PdfPageObjectCommon, PdfPageAnnotationCommon, 
PdfPageObjectType, PdfPagePathObject};
use std::path::PathBuf;
use std::collections::HashSet;
use std::collections::HashMap;
use base64::Engine;
use std::sync::{Arc, Mutex};
use std::process::Command;
use std::fs;
use notify::{Watcher, RecursiveMode};
use image::{GenericImageView, GenericImage};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// 根据操作系统嵌入不同的库文件
#[cfg(target_os = "linux")]
const PDFIUM_LIB_BYTES: &[u8] = include_bytes!("../assets/libpdfium.so");

#[cfg(target_os = "windows")]
const PDFIUM_LIB_BYTES: &[u8] = include_bytes!("../assets/pdfium.dll");

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AppConfig {
    pub favorite_files: Vec<std::path::PathBuf>,
    pub favorite_folders: Vec<std::path::PathBuf>,

		#[serde(default)] 
    pub show_full_path: bool,

		#[serde(default = "default_editor_command")]
		pub editor_command: String,

    #[serde(default)]
    pub ocr_models: Vec<OcrModelConfig>,
    #[serde(default)]
    pub current_ocr_model_name: String,
}

#[cfg(target_os = "windows")]
fn default_editor_command() -> String {
    "gvim.exe --remote-silent +{line} {file}".to_string()
}

#[cfg(target_os = "linux")]
fn default_editor_command() -> String {
    "gvim --remote-silent +{line} {file}".to_string()
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct OcrModelConfig {
    pub name: String,
    pub model_id: String,
    pub provider: String,
    pub api_token: String,
    pub api_url: String,
}

impl Default for AppConfig {
	fn default() -> Self {
		Self {
			favorite_files: vec![],
			favorite_folders: vec![],
			show_full_path: false,
			editor_command: default_editor_command(),
			ocr_models: vec![
				OcrModelConfig {
					name: "GLM-OCR".to_string(),
					model_id: "GLM-OCR-Q8_0".to_string(),
					provider: "local".to_string(),
					api_token: "".to_string(),
					api_url: "".to_string(),
				},
			],
			current_ocr_model_name: "GLM-OCR".to_string(),
		}
	}
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct AppHistory {
    pub last_pages: HashMap<String, usize>,

		#[serde(default)]
    pub order: Vec<String>,
}

enum ShortcutAction {
    None,
    PrevPage,
    NextPage,
		Prev10Pages, 
    Next10Pages, 
		FirstPage,  
    LastPage,   
    ClosePreview,
    CopyImage,
    SaveImage,
		ZoomIn,      
    ZoomOut,     
    ResetZoom, 
		ResetApp,
		ToggleOcrWindow,
		ForceOpenEmptyOcr,
		ScrollLeft,
		ScrollRight,
		GotoPage(usize),
}

struct CachedLink {
    rect: egui::Rect,
    destination: Option<usize>, 
    uri: Option<String>,       
}
#[derive(Clone)]
struct EraseStroke {
    points: Vec<egui::Pos2>,
    brush_size: f32,
		color: egui::Color32,
}
impl Default for EraseStroke {
	fn default() -> Self {
		Self {
			points: Vec::new(),
			brush_size: 20.0,
			color: egui::Color32::WHITE,
		}
	}
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolMode {
    Pen,
    Eraser,
}
#[derive(Clone, Copy, Debug)]
struct StrokePoint {
    pos: egui::Pos2, // 归一化坐标 (0.0 - 1.0)
    width: f32,      // 该点的最终物理宽度
}
#[derive(Clone)]
struct Stroke {
    points: Vec<StrokePoint>, // 这一条线包含的所有点
    color: egui::Color32,     // 这一条线专属的颜色
}
enum ColorShape {
    Circle,
    Rect,
}
#[derive(Clone)]
enum Annotation {
    Pen(Stroke),
    Eraser(EraseStroke),
}
//字段
struct PdfApp {
    //pdfium: Pdfium,
		pdfium: &'static Pdfium,
		config: AppConfig,
		last_applied_title: String,
    pdf_doc: Option<PdfDocument<'static>>,
    current_page: usize,
    texture: Option<egui::TextureHandle>,
		cropped_tex: Option<egui::TextureHandle>, 
		last_cropped_image: Option<image::RgbaImage>,
		last_ocr_image: Option<image::RgbaImage>,
		pdf_name: Option<String>,
		pdf_path: Option<PathBuf>,
		target_dpi: f32,
		view_scale: f32,
		zoom_factor: f32,
		last_opened_dir: Option<std::path::PathBuf>,
		drag_start_local: Option<egui::Pos2>,     
    selection_rect_local: Option<egui::Rect>,
		//favorite_files: Vec<PathBuf>,
		//favorite_folders: Vec<PathBuf>,
		//show_full_path: bool,
		//editor_command: String,
		rotations: Vec<f32>,
		show_help_window: bool,
		last_rendered_page: usize,
    last_rendered_angle: f32,
		show_export_window: bool,
		export_range_text: String,
		show_merge_window: bool,
		merge_file_list: Vec<std::path::PathBuf>,
		link_cache: Vec<CachedLink>,
		last_link_rect: Option<egui::Rect>,
		last_link_page: Option<usize>,
		history: AppHistory,
		ocr_result: Arc<Mutex<Option<String>>>,
		is_ocr_loading: Arc<Mutex<bool>>,
		ocr_api_url: String,
    ocr_api_token: String,
    ocr_model_name: String,
		ocr_provider: String,
		show_ocr_window: bool,
		current_latex: Option<String>,
    preview_texture: Option<egui::TextureHandle>,       // 主线程负责转为纹理
    is_preview_loading: Arc<Mutex<bool>>,               // 预览专用的加载状态
		show_preview_window: bool,
		latex_error: Arc<Mutex<Option<String>>>,
		preview_image: Option<image::RgbaImage>,
		scroll_delta: egui::Vec2,
		goto_buffer: String, // 存储数字前缀
		pending_g: bool,
		last_g_time: f64,
		needs_reload: Arc<Mutex<bool>>, // 使用 Mutex 标记是否需要重新加载
		watcher: Option<notify::RecommendedWatcher>,
		reload_retries: u8,
		last_pdf_width: f32,
		last_pdf_height: f32,
		is_edit_mode: bool,
		current_pen_stroke: Vec<StrokePoint>,
		current_eraser_stroke: Vec<egui::Pos2>,
		tool_mode: ToolMode,
		pen_color: egui::Color32,
		pen_size: f32,
		eraser_size: f32,
		eraser_color: egui::Color32,
		annotations: std::collections::BTreeMap<usize, Vec<Annotation>>,
		last_pressure: f32,
		last_time_stamp: f32,
}

fn load_config() -> AppConfig {
    let path = config_path();

    if let Ok(data) = std::fs::read_to_string(&path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        AppConfig::default()
    }
}
fn save_config(cfg: &AppConfig) {
    let path = config_path();

    if let Ok(data) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, data);
    }
}
fn config_path() -> std::path::PathBuf {
    let mut dir = dirs::config_dir().unwrap();
    dir.push("vectorsnap");
    std::fs::create_dir_all(&dir).ok();
    dir.push("config.json");
    dir
}
fn history_path() -> std::path::PathBuf {
    //let mut dir = dirs::config_dir().unwrap();
		let mut dir = dirs::data_dir().expect("Data directory not found");
    dir.push("vectorsnap");
    dir.push("history.json");
    dir
}
fn load_history() -> AppHistory {
	let path = history_path();
	if let Ok(data) = std::fs::read_to_string(&path) {
		serde_json::from_str(&data).unwrap_or_default()
	} else {
		AppHistory::default()
	}
}
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 加载文泉驿
    fonts.font_data.insert(
        "wqy".to_owned(),
        //egui::FontData::from_static(include_bytes!("../assets/wqy-microhei.ttc")).into(),
        egui::FontData::from_static(include_bytes!("../assets/MiSans-Regular.ttf")).into(),
    );

    // 将 WQY 插入到默认字体家族的末尾
    // 这样：'A' 会在默认字体里找到并显示；'中' 在默认字体里找不到，会跳到 WQY 里找
    fonts.families.get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .push("wqy".to_owned()); // 使用 .push() 放在最后

    fonts.families.get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("wqy".to_owned());

    ctx.set_fonts(fonts);
}
fn get_target_lib_path() -> std::path::PathBuf {
    use std::path::PathBuf;
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").expect("Home dir not found");
        PathBuf::from(home).join(".local/lib/libpdfium.so")
    }
    #[cfg(target_os = "windows")]
    {
        let app_data = std::env::var("LOCALAPPDATA").expect("AppData not found");
        PathBuf::from(app_data).join("VectorSnap").join("bin").join("pdfium.dll")
    }
}
fn extract_pdfium_to_local(path: &std::path::PathBuf) {
    use std::fs;
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            let _ = fs::create_dir_all(parent);
        }
    }
    let _ = fs::write(path, PDFIUM_LIB_BYTES);
}
//初始化
impl PdfApp {
	pub fn new(_cc: &eframe::CreationContext<'_>, path: Option<std::path::PathBuf>) -> Self {
		// 1. 第一步：尝试直接绑定系统已有的库
		let bindings = Pdfium::bind_to_system_library()
			.or_else(|_| {
				// 2. 第二步：系统没有，则尝试绑定我们自己的标准路径库
				let local_path = get_target_lib_path();
				Pdfium::bind_to_library(local_path.to_str().unwrap())
					.or_else(|_| {
						// 3. 第三步：标准路径也没有（或坏了），解压嵌入的库再试一次
						println!("System library not found. Extracting embedded library...");
						extract_pdfium_to_local(&local_path);
						Pdfium::bind_to_library(local_path.to_str().unwrap())
					})
			})
		.expect("无法加载 Pdfium！");

		// 4. 将 Pdfium 实例放入堆并“泄露”以获得静态生命周期
		//let pdfium = Box::leak(Box::new(Pdfium::default()));
		let pdfium = Box::leak(Box::new(Pdfium::new(bindings)));

		let config = load_config();

		let (api_url, api_token, model_id, provider) = config.ocr_models
			.iter()
			.find(|m| m.name == config.current_ocr_model_name) // 改用 name 匹配
			.map(|m| (
					m.api_url.clone(), 
					m.api_token.clone(), 
					m.model_id.clone(), 
					m.provider.clone()
			))
			.unwrap_or_else(|| {
				(
					"".to_string(), 
					"".to_string(), 
					"GLM-OCR-Q8_0".to_string(), 
					"local".to_string()
				)
			});

		let mut app = Self {
			pdfium,
			last_applied_title: "VectorSnap".to_string(),
			pdf_doc: None,
			current_page: 0,
			texture: None,
			cropped_tex: None,
			last_cropped_image: None,
			last_ocr_image: None,
			pdf_name: None,
			pdf_path: None,
			target_dpi: 600.0,
			view_scale: 1.0,
			zoom_factor: 1.0,
			last_opened_dir: None,
			drag_start_local: None,
			selection_rect_local: None,
			//favorite_files: config.favorite_files.clone(),
			//favorite_folders: config.favorite_folders.clone(),
			//show_full_path: config.show_full_path.clone(),
			//editor_command: config.editor_command.clone(),
			config,
			rotations: Vec::new(),
			show_help_window: false,
			last_rendered_page: 999999, 
			last_rendered_angle: 0.0,
			show_export_window: false,
			export_range_text: "".to_string(),
			show_merge_window: false,
			merge_file_list: Vec::new(),
			link_cache: Vec::new(),
			last_link_rect: None,
			last_link_page: None,
			history: load_history(),
			ocr_result: Arc::new(Mutex::new(None)),
			is_ocr_loading: Arc::new(Mutex::new(false)),
			//ocr_api_url: "".to_string(),
			//ocr_api_token: "".to_string(),
			//ocr_model_name: "GLM-OCR-Q8_0".to_string(),
			//ocr_provider: "local".to_string(),
			ocr_api_url: api_url,
			ocr_api_token: api_token,
			ocr_model_name: model_id,
			ocr_provider: provider,
			show_ocr_window: false,
			current_latex: None,
			is_preview_loading: Arc::new(Mutex::new(false)),
			preview_texture: None,
			show_preview_window: false,
			latex_error: Arc::new(Mutex::new(None)),
			preview_image: None,
			scroll_delta: egui::Vec2::ZERO,
			goto_buffer: String::new(),
			pending_g: false,
			last_g_time: 0.0,
			needs_reload: Arc::new(Mutex::new(false)),
			watcher: None,
			reload_retries: 0,
			last_pdf_width: 0.0,
			last_pdf_height: 0.0,
			is_edit_mode: false,
			current_pen_stroke: Vec::new(),
			current_eraser_stroke: Vec::new(),
			tool_mode: ToolMode::Eraser,
			pen_color: egui::Color32::BLACK,
			pen_size: 2.0,
			eraser_size: 20.0,
			eraser_color: egui::Color32::WHITE,
			annotations: std::collections::BTreeMap::new(),
			last_pressure: 0.0,
			last_time_stamp: 0.0,
		};

		// 如果启动参数里有路径，直接调用加载函数
		if let Some(p) = path {
			app.load_pdf_path(p);
		}

		app
	}
}
//记住历史位置
impl PdfApp {
	fn save_history(&self) {
		let path = history_path();
		if let Ok(data) = serde_json::to_string_pretty(&self.history) {
			let _ = std::fs::write(path, data);
		}
	}

	// Call this inside unload_pdf or when switching files
	fn record_current_position(&mut self) {
		if let Some(path) = &self.pdf_path {
			let path_str = path.to_string_lossy().to_string();
			let max_records = 1000;
			// 1. 更新页码
			self.history.last_pages.insert(path_str.clone(), self.current_page);
			// 2. 维护顺序（去重并置顶）
			// 如果路径已在列表中，先删掉旧的（保证它等会能排到最后，即最新）
			self.history.order.retain(|x| x != &path_str);
			// 将当前路径推入末尾（代表它是最近访问的）
			self.history.order.push(path_str);
			// 3. 超过最大值时，删除最旧的
			while self.history.order.len() > max_records {
				// 移除最前面（最旧）的一个
				let oldest_path = self.history.order.remove(0);
				// 同步从 HashMap 中删除
				self.history.last_pages.remove(&oldest_path);
			}
			// 4. 保存到磁盘
			self.save_history();
		}
	}
}
//快捷键
impl PdfApp {
	fn handle_shortcuts(&mut self, ctx: &egui::Context) {
		let mut action = ShortcutAction::None;


		// 1. 获取当前焦点状态
		// 如果用户正在 TextEdit 中打字，has_focus 为 true
		let has_focus = ctx.memory(|mem| mem.focused().is_some());

		let current_time = ctx.input(|i| i.time);
		// 如果距离上次按 g 超过 1.0 秒，自动重置 pending 状态
    if self.pending_g && (current_time - self.last_g_time) > 1.0 {
        self.pending_g = false;
				self.goto_buffer.clear();
    }

		ctx.input_mut(|i| {
			// ---------- 第一优先级：全局通用快捷键 (无论是否有焦点都响应) ----------

			// Esc 键始终用于关闭预览或 OCR 窗口
			if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
				action = ShortcutAction::ClosePreview;
				return; 
			}

			// Ctrl + S 始终允许保存截图
			if i.modifiers.ctrl && i.key_pressed(egui::Key::S) {
				if self.last_cropped_image.is_some() {
					action = ShortcutAction::SaveImage;
					return;
				}
			}

		// 1. 放大：满足 (Ctrl + =) 或 (无 Ctrl + 无焦点 + =)
			if (i.modifiers.ctrl && (i.key_pressed(egui::Key::Equals) || i.key_pressed(egui::Key::Plus))) 
				|| (!has_focus && i.consume_key(egui::Modifiers::NONE, egui::Key::Equals))
					|| (!has_focus && i.consume_key(egui::Modifiers::NONE, egui::Key::Plus)) 
			{
				if i.modifiers.ctrl {
					i.consume_key(egui::Modifiers::CTRL, egui::Key::Equals);
					i.consume_key(egui::Modifiers::CTRL, egui::Key::Plus);
				}
				action = ShortcutAction::ZoomIn;
				return; 
			}

			// 2. 缩小：满足 (Ctrl + -) 或 (无 Ctrl + 无焦点 + -)
			if (i.modifiers.ctrl && i.key_pressed(egui::Key::Minus))
				|| (!has_focus && i.consume_key(egui::Modifiers::NONE, egui::Key::Minus))
			{
				if i.modifiers.ctrl {
					i.consume_key(egui::Modifiers::CTRL, egui::Key::Minus);
				}
				action = ShortcutAction::ZoomOut;
				return; 
			}

			if !has_focus && i.consume_key(egui::Modifiers::NONE, egui::Key::A) {
				action = ShortcutAction::ResetZoom;
				return;
			}

			// 3. 重置：满足 (Ctrl + 0) 
			if i.modifiers.ctrl && i.key_pressed(egui::Key::Num0) {
				i.consume_key(egui::Modifiers::CTRL, egui::Key::Num0);
				action = ShortcutAction::ResetZoom;
				return; 
			}

			// ---------- 第二优先级：焦点拦截 ----------

			// 如果当前正在输入 (has_focus)，拦截掉剩下的所有单键快捷键
			// 这样按 J/K 就不会翻页，Ctrl+C 也会由 TextEdit 自己处理
			if has_focus {
				return; 
			}

			// ---------- 第三优先级：仅在非输入状态下的快捷键 ----------
			// 旋转页面
			if i.consume_key(egui::Modifiers::SHIFT, egui::Key::R) {
				self.rotate_current_page(-90.0);
				return;
			}
			if i.consume_key(egui::Modifiers::NONE, egui::Key::R) {
				self.rotate_current_page(90.0);
				return;
			}

			if i.consume_key(egui::Modifiers::SHIFT, egui::Key::G) {
				action = ShortcutAction::LastPage;
				self.goto_buffer.clear();
				self.pending_g = false;
				return; // 立即返回，防止流向下方数字或小写 g 逻辑
			}
			// --- 1. 数字键捕获 (0-9) ---
			for n in 0..=9 {
				let key = match n {
					0 => egui::Key::Num0, 1 => egui::Key::Num1, 2 => egui::Key::Num2,
					3 => egui::Key::Num3, 4 => egui::Key::Num4, 5 => egui::Key::Num5,
					6 => egui::Key::Num6, 7 => egui::Key::Num7, 8 => egui::Key::Num8,
					9 => egui::Key::Num9, _ => unreachable!(),
				};
				if i.consume_key(egui::Modifiers::NONE, key) {
					self.goto_buffer.push_str(&n.to_string());
					self.pending_g = false; // 输入数字时，g 的连击计数重置
					return;
				}
			}

			// --- 2. 核心：处理 g 键逻辑 ---
			if i.consume_key(egui::Modifiers::NONE, egui::Key::G) {
				if self.pending_g {
					// 只有当这是第二个 'g' 时，才执行跳转
					if !self.goto_buffer.is_empty() {
						// 逻辑 A: [数字]gg -> 跳转到指定页 (例如 20gg)
						if let Ok(page_num) = self.goto_buffer.parse::<usize>() {
							action = ShortcutAction::GotoPage(page_num.saturating_sub(1));
						}
					} else {
						// 逻辑 B: 纯 gg -> 跳转首页
						action = ShortcutAction::FirstPage;
					}
					// 动作完成，重置所有状态
					self.goto_buffer.clear();
					self.pending_g = false;
				} else {
					// 这是第一个 'g'，只记录状态，不动
					self.pending_g = true;
					// 更新时间
					self.last_g_time = current_time;
				}
				return;
			}
			// --- 3. 状态清理 (Vim 行为) ---
			if i.key_pressed(egui::Key::Escape) 
				|| i.key_pressed(egui::Key::J) 
					|| i.key_pressed(egui::Key::K) 
					|| i.key_pressed(egui::Key::PageUp)
					|| i.key_pressed(egui::Key::PageDown)
			{
				self.goto_buffer.clear();
				self.pending_g = false;
			}
			if i.pointer.any_pressed() {
				self.goto_buffer.clear();
				self.pending_g = false;
			}



			// Ctrl + C (复制截图)
			if i.events.iter().any(|e| matches!(e, egui::Event::Copy)) {
				if self.last_cropped_image.is_some() {
					action = ShortcutAction::CopyImage;
					return; 
				}
			}

			if i.key_pressed(egui::Key::H) || i.key_pressed(egui::Key::ArrowLeft) {
				action = ShortcutAction::ScrollLeft;
				return; 
			} else if i.key_pressed(egui::Key::L) || i.key_pressed(egui::Key::ArrowRight) {
				action = ShortcutAction::ScrollRight;
				return; 
			}

			// 翻页逻辑 (J/K/G/Home/End)
			let up_10 = i.consume_key(egui::Modifiers::SHIFT, egui::Key::K);
			let down_10 = i.consume_key(egui::Modifiers::SHIFT, egui::Key::J);

			let up = i.consume_key(egui::Modifiers::NONE, egui::Key::K)
				|| i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp)
				|| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);

			let down = i.consume_key(egui::Modifiers::NONE, egui::Key::J)
				|| i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown)
				|| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);

			let home = i.consume_key(egui::Modifiers::NONE, egui::Key::Home);

			let end = i.consume_key(egui::Modifiers::NONE, egui::Key::End);

			if up_10 { 
				action = ShortcutAction::Prev10Pages; 
				return; 
			} else if down_10 { 
				action = ShortcutAction::Next10Pages; 
				return; 
			} else if up { 
				action = ShortcutAction::PrevPage; 
				return; 
			} else if down { 
				action = ShortcutAction::NextPage; 
				return; 
			} else if home { 
				action = ShortcutAction::FirstPage; 
				return; 
			} else if end { 
				action = ShortcutAction::LastPage; 
				return; 
			}

			if i.consume_key(egui::Modifiers::NONE, egui::Key::V) {
					//action = ShortcutAction::ToggleOcrWindow;
				if i.modifiers.shift {
					// Shift + V：强制开启空白面板
					action = ShortcutAction::ForceOpenEmptyOcr;
					return; 
				} else {
					// 纯 V：切换现有结果
					action = ShortcutAction::ToggleOcrWindow;
					return; 
				}
			}

			// 退出程序快捷键 Q
			if i.consume_key(egui::Modifiers::NONE, egui::Key::Q) {
				action = ShortcutAction::ResetApp;
				return; 
			}
		});

		let scroll_step = 40.0;  // 滚动步长
		match action {
			ShortcutAction::PrevPage => {
				if self.current_page > 0 {
					self.current_page -= 1;
					self.texture = None;
					ctx.request_repaint();
				}
			}

			ShortcutAction::NextPage => {
				if let Some(doc) = &self.pdf_doc {
					if self.current_page + 1 < doc.pages().len() as usize {
						self.current_page += 1;
						self.texture = None;
						ctx.request_repaint();
					}
				}
			}

			ShortcutAction::FirstPage => {
				if self.current_page != 0 {
					self.current_page = 0;
					self.texture = None;
					ctx.request_repaint();
				}
			}
			ShortcutAction::GotoPage(target_page) => {
				if let Some(doc) = &self.pdf_doc {
					// 核心安全检查：确保页码不超标
					let total_pages = doc.pages().len() as usize;
					let safe_page = target_page.min(total_pages.saturating_sub(1));

					if self.current_page != safe_page {
						self.current_page = safe_page;
						// 关键：清空当前纹理，让程序加载新页面的渲染
						self.texture = None; 
						ctx.request_repaint();
					}
				}
			}
			ShortcutAction::LastPage => {
				if let Some(doc) = &self.pdf_doc {
					let last = (doc.pages().len() as usize).saturating_sub(1);
					if self.current_page != last {
						self.current_page = last;
						self.texture = None;
						ctx.request_repaint();
					}
				}
			}

			ShortcutAction::Prev10Pages => {
				let new_page = self.current_page.saturating_sub(10);
				if new_page != self.current_page {
					self.current_page = new_page;
					self.texture = None;
					ctx.request_repaint();
				}
			}
			ShortcutAction::Next10Pages => {
				if let Some(doc) = &self.pdf_doc {
					let total = doc.pages().len() as usize;

					if total > 0 {
						let new_page = (self.current_page + 10).min(total.saturating_sub(1));

						if new_page != self.current_page {
							self.current_page = new_page;
							self.texture = None;
							ctx.request_repaint();
						}
					}
				}
			}

			ShortcutAction::ClosePreview => {
				if self.show_preview_window {
					self.show_preview_window = false;
					//self.preview_texture = None;
				} else if self.show_ocr_window {
					self.show_ocr_window = false;
				} else {
					self.cropped_tex = None;
					self.last_cropped_image = None;
				}
			}

			ShortcutAction::ToggleOcrWindow => {
				if self.current_latex.is_some() {
					self.show_ocr_window = !self.show_ocr_window;
				}
			}

			ShortcutAction::ForceOpenEmptyOcr => {
        // 冷启动：不管之前有没有，直接覆盖成空字符串并强制显示
        self.current_latex = Some("".to_string());
        self.show_ocr_window = true;
        //ctx.request_repaint();
				let input_id = egui::Id::new("latex_input_field");
				ctx.memory_mut(|mem| mem.request_focus(input_id));
    }

			ShortcutAction::CopyImage => {
				if let Some(img) = &self.last_cropped_image {
					copy_image_to_clipboard(img); 
				}
				self.cropped_tex = None;
				self.last_cropped_image = None;
			}

			ShortcutAction::SaveImage => {
				//self.save_image_with_format("png");
				let img = self.last_cropped_image.clone();
				self.save_generic_image(img, "Crop", "png");
			}

			ShortcutAction::ZoomIn => {
				self.zoom_factor = (self.zoom_factor * 1.1).clamp(0.1, 10.0);
			}
			ShortcutAction::ZoomOut => {
				self.zoom_factor = (self.zoom_factor / 1.1).clamp(0.1, 10.0);
			}
			ShortcutAction::ResetZoom => {
				self.zoom_factor = 1.0;
			}
			ShortcutAction::ResetApp => {
				self.unload_pdf();
				ctx.request_repaint(); // 强制重绘以显示初始欢迎界面
			}

			ShortcutAction::ScrollLeft => { self.scroll_delta.x -= scroll_step; }
			ShortcutAction::ScrollRight => { self.scroll_delta.x += scroll_step; }

			ShortcutAction::None => {}
		}
	}
}
//菜单栏
impl PdfApp {
	fn render_top_panel(&mut self, ctx: &egui::Context) {
		egui::TopBottomPanel::top("controls").show(ctx, |ui| {
			ui.horizontal(|ui| {
				if ui.button("📁 Open PDF").clicked() {
					let mut dialog = rfd::FileDialog::new().add_filter("PDF files", &["pdf", "PDF"]);

					if let Some(dir) = &self.last_opened_dir {
						dialog = dialog.set_directory(dir);
					}

					if let Some(path) = dialog.pick_file() {
						self.load_pdf_path(path); 
					}
				}

				ui.separator();

				if let Some(doc) = &self.pdf_doc {
					let total = doc.pages().len() as usize;

					ui.horizontal(|ui| {
						// --- 1. 上一页按钮 ---
						// 只有当前页码大于 0 才能点
						if ui.button("⬅ Prev").clicked() && self.current_page > 0 {
							self.current_page -= 1;
							self.texture = None; // 清除纹理触发重新渲染
						}

						// --- 2. 页码显示 ---
						//ui.label(format!("Page {} of {}", self.current_page + 1, total));
						let mut display_page = self.current_page + 1;

						ui.label("Page");
						let res = ui.add(
							egui::DragValue::new(&mut display_page)
							.range(1..=total)     // 限制输入范围
							.speed(0.1)           // 拖拽时的感应速度
						);
						ui.label(format!("of {}", total));

						// 如果用户输入了新数字并按回车，或者拖动了数字
						if res.changed() {
							self.current_page = display_page.saturating_sub(1);
							self.texture = None; // 触发重新渲染
						}

						// --- 3. 下一页按钮 ---
						// 只有当前页码还没到最后一页才能点
						if ui.button("Next ➡").clicked() && self.current_page + 1 < total {
							self.current_page += 1;
							self.texture = None; // 清除纹理触发重新渲染
						}


						if !self.is_edit_mode {
						ui.separator();
						ui.label("Crop Quality:");
						egui::ComboBox::from_id_salt("quality_scale")
							.selected_text(format!("{}dpi", self.target_dpi)) // 显示当前选中的值
							.width(40.0) // 固定宽度，让界面更整齐
							.show_ui(ui, |ui| {
								ui.selectable_value(&mut self.target_dpi, 150.0, "150");
								ui.selectable_value(&mut self.target_dpi, 200.0, "200");
								ui.selectable_value(&mut self.target_dpi, 300.0, "300");
								ui.selectable_value(&mut self.target_dpi, 400.0, "400");
								ui.selectable_value(&mut self.target_dpi, 600.0, "600");
								ui.selectable_value(&mut self.target_dpi, 800.0, "800");
								ui.selectable_value(&mut self.target_dpi, 1200.0, "1200");
								ui.selectable_value(&mut self.target_dpi, 1600.0, "1600");
							});
						}

						ui.separator();
						let edit_label = if self.is_edit_mode { "Stop Editing" } else { "Edit Mode" };
						if ui.selectable_label(self.is_edit_mode, edit_label).clicked() {
							self.is_edit_mode = !self.is_edit_mode;
							//self.current_pen_stroke.clear();
							//self.current_eraser_stroke.clear();
						}
						if self.is_edit_mode {
							ui.horizontal(|ui| {
								if ui.selectable_label(self.tool_mode == ToolMode::Pen, "✏ Pen").clicked() {
									self.tool_mode = ToolMode::Pen;
								}
								if ui.selectable_label(self.tool_mode == ToolMode::Eraser, "Eraser").clicked() {
									self.tool_mode = ToolMode::Eraser;
								}
								ui.menu_button("Colors", |ui| {
									ui.group(|ui| {
										ui.label("✏ Pen");
										ui.horizontal(|ui| {
											ui.label("Size:");
											ui.add(egui::Slider::new(&mut self.pen_size, 1.0..=10.0));
										});
										let pen_colors = [
											egui::Color32::BLACK,
											egui::Color32::RED,
											egui::Color32::BLUE,
											egui::Color32::from_rgb(0, 100, 0),
											egui::Color32::from_rgb(139, 69, 19),
										];
										color_selector(ui, &mut self.pen_color, &pen_colors, ColorShape::Circle);
									});
									ui.separator();
									ui.group(|ui| {
										ui.label("Eraser");
										ui.horizontal(|ui| {
											ui.label("Size:");
											ui.add(egui::Slider::new(&mut self.eraser_size, 5.0..=50.0));
										});
										let eraser_colors = [
											egui::Color32::WHITE,
											egui::Color32::from_rgb(240, 240, 240), 
											egui::Color32::BLACK,
											egui::Color32::YELLOW, 
											egui::Color32::from_rgb(75, 0, 130),
										];
										color_selector(ui, &mut self.eraser_color, &eraser_colors, ColorShape::Rect);
									});
								});

								if ui.button("Undo").clicked() {
									if let Some(annos) = self.annotations.get_mut(&self.current_page) {
										annos.pop(); // 弹出最后添加的一个 Annotation
										if annos.is_empty() {
											self.annotations.remove(&self.current_page);
										}
									}
								}
								if ui.button("Clear").clicked() {
									self.annotations.remove(&self.current_page);
								}
								if ui.button("Save").clicked() {
									let dialog = rfd::FileDialog::new()
										.add_filter("PDF files", &["pdf"])
										.set_file_name("modified.pdf");

									if let Some(path) = dialog.save_file() {
										if let Err(e) = self.save_as_pdf(path) {
											eprintln!("Failed to save PDF: {:?}", e);
										}
									}
								}
							});
						}

						ui.separator();
						if ui.button("Export").clicked() {
							self.show_export_window = true;
							ui.close_kind(egui::UiKind::Menu);
						}

						ui.separator();
						ui.menu_button("More", |ui| {
							let qualities = [
								(1.0, "Fast"),
								(1.25, "Balanced"),
								(1.5, "High-Q"),
								(2.0, "HiDPI"),
							];

							let current_quality_name = qualities.iter()
								.find(|(val, _)| (*val - self.view_scale).abs() < 0.01) // 浮点数安全比较
								.map(|(_, name)| *name)
								.unwrap_or("Custom");

							ui.menu_button(format!("View Quality: {}", current_quality_name), |ui| {
								for (value, label) in qualities {
									// 使用 selectable_label，点击后逻辑更清晰
									if ui.selectable_label(self.view_scale == value, label).clicked() {
										self.view_scale = value;
										self.texture = None; // 触发重绘
										ui.close_kind(egui::UiKind::Menu); // 选中后自动关闭子菜单
									}
								}
							});

							ui.separator();
							//let current_model_name = self.config.ocr_models.iter()
							//	.find(|m| m.model_id == self.config.current_ocr_model_name)
							//	.map(|m| m.name.as_str())
							//	.unwrap_or("Select Model");
							//ui.menu_button(format!("OCR Model: {}", current_model_name), |ui| {
							//	for model in &self.config.ocr_models {
							//		let is_selected = model.model_id == self.config.current_ocr_model_name;
							//		if ui.selectable_label(is_selected, &model.name).clicked() {
							//			self.config.current_ocr_model_name = model.model_id.clone();
							//			self.ocr_model_name = model.model_id.clone();
							//			self.ocr_api_token = model.api_token.clone();
							//			self.ocr_api_url = model.api_url.clone();
							//			self.ocr_provider = model.provider.clone();
							//			//self.config.save(); // 自动保存选择
							//			ui.close_kind(egui::UiKind::Menu); 
							//		}
							//	}
							//});
							let current_display_name = self.config.ocr_models.iter()
								.find(|m| m.name == self.config.current_ocr_model_name)
								.map(|m| m.name.as_str())
								.unwrap_or("Select Model");
							ui.menu_button(format!("OCR Model: {}", current_display_name), |ui| {
								for model in &self.config.ocr_models {
									let is_selected = model.name == self.config.current_ocr_model_name;
									if ui.selectable_label(is_selected, &model.name).clicked() {
										self.config.current_ocr_model_name = model.name.clone();
										self.ocr_model_name = model.model_id.clone();
										self.ocr_api_token = model.api_token.clone();
										self.ocr_api_url = model.api_url.clone();
										self.ocr_provider = model.provider.clone();
										//self.save_all_config();// 保存配置
										ui.close_kind(egui::UiKind::Menu); 
									}
								}
							});

							ui.separator();
							if ui.button("Rotate CW 90°").clicked() {
								self.rotate_current_page(90.0);
							}
							if ui.button("Rotate CCW 90°").clicked() {
								self.rotate_current_page(-90.0);
							}

							ui.separator();
							if ui.button("🔍 Zoom In (+)").clicked() {
								// 限制最大放大到 10 倍
								self.zoom_factor = (self.zoom_factor * 1.1).clamp(0.2, 5.0);
								//self.texture = None;
							}
							if ui.button("🔍 Zoom Out (-)").clicked() {
								// 限制最小缩小到 0.1 倍
								self.zoom_factor = (self.zoom_factor / 1.1).clamp(0.2, 5.0);
								//self.texture = None;
							}
							if ui.button("🔄 Reset Zoom").clicked() {
								self.zoom_factor = 1.0;
								//self.texture = None;
							}

							ui.separator();
							if ui.button("Help & Shortcuts").clicked() {
								self.show_help_window = true;
								ui.close_kind(egui::UiKind::Menu);
							}
						});

					});

				} else {
					// 未打开任何 PDF，显示合并工具
					if ui.button("Merge Files to PDF").clicked() {
						self.show_merge_window = true;
					}
				}

			});
		});
	}
}
// 颜色选择器
fn color_selector(
	ui: &mut egui::Ui,
	current_color: &mut egui::Color32,
	available_colors: &[egui::Color32],
	shape: ColorShape,
) {
	ui.horizontal(|ui| {
		ui.label("Color:");
		for &c in available_colors {
			let (rect, response) = ui.allocate_exact_size(
				egui::vec2(18.0, 18.0),
				egui::Sense::click(),
			);

			let is_selected = *current_color == c;
			let painter = ui.painter();

			// 绘制背景形状
			match shape {
				ColorShape::Circle => {
					painter.circle_filled(rect.center(), 7.0, c);
				}
				ColorShape::Rect => {
					painter.rect_filled(rect.shrink(2.0), 2.0, c);
				}
			}

			// 绘制边框（选中效果）
			let stroke_color = if is_selected {
				egui::Color32::LIGHT_BLUE
			} else {
				egui::Color32::GRAY
			};
			let stroke_width = if is_selected { 2.0 } else { 1.0 };

			match shape {
				ColorShape::Circle => {
					painter.circle_stroke(rect.center(), 7.0, egui::Stroke::new(stroke_width, stroke_color));
				}
				ColorShape::Rect => {
					painter.rect_stroke(
						rect.shrink(2.0),
						2.0,
						egui::Stroke::new(stroke_width, stroke_color),
						egui::StrokeKind::Outside,
					);
				}
			}

			if response.clicked() {
				*current_color = c;
			}
			response.on_hover_text(format!("R:{} G:{} B:{}", c.r(), c.g(), c.b()));
		}
	});
}
// 帮助窗口
impl PdfApp {
    fn render_help_window(&mut self, ctx: &egui::Context) {
        let mut is_open = self.show_help_window;
        let mut user_clicked_close = false;

        egui::Window::new("📖 About VectorSnap")
            .open(&mut is_open) 
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::RIGHT_TOP, [-20.0, 40.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("VectorSnap - Educator's PDF Toolkit");
                    ui.label("专为教学而设计，助力老师轻松获取高清素材");
                    ui.label("Designed for educators to capture high-res materials");
                });
                
                ui.add_space(15.0);
                ui.separator();
                ui.add_space(15.0);

                // --- 1. 渲染说明卡片 (Quality Explanation Card) ---
                egui::Frame::group(ui.style())
                    .fill(ui.visuals().faint_bg_color)
                    .corner_radius(5.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
													ui.set_width(ui.available_width());
                            ui.strong("⚙️渲染说明 (Quality Explanation)");
                            ui.add_space(6.0);
                            
                            ui.label("• 预览质量 (View Quality): 影响主窗口缩放流畅度");
                            ui.label("  (Affects preview smoothness)");
                            
                            ui.add_space(4.0);
                            ui.label("• 截图质量 (Crop DPI): 600dpi+ 即可导出超清图片");
                            ui.label("  (Sets high resolution for snippets)");
                            
                            ui.add_space(8.0);
                            ui.colored_label(
                                egui::Color32::from_rgb(200, 180, 0), 
                                "💡 预览模糊并不影响截图的最终清晰度\n   (Preview blur won't affect crop sharpness)"
                            );
                        });
                    });

                ui.add_space(15.0);

                // --- 2. 快捷键卡片 (Shortcuts Card) ---
                egui::Frame::group(ui.style())
                    .fill(ui.visuals().faint_bg_color)
                    .corner_radius(5.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
											ui.set_width(ui.available_width());
                        ui.vertical(|ui| {
                            ui.strong("⌨️快捷键 (Shortcuts)");
                            ui.add_space(10.0);

                            egui::Grid::new("shortcuts_grid")
                                .striped(true)
                                .num_columns(2)
                                .spacing([40.0, 10.0])
                                .show(ui, |ui| {
                                    ui.label("上一页/下一页 (Prev/Next)");
                                    ui.label("↑ / ↓  or  K / J");
                                    ui.end_row();

                                    ui.label("前10页/后10页 (Prev 10/Next 10)");
                                    ui.label("Shift +  [K / J]");
                                    ui.end_row();

                                    ui.label("首页 (First page)");
                                    ui.label("0 or ^");
                                    ui.end_row();

                                    ui.label("末页 (Last page)");
                                    ui.label("G");
                                    ui.end_row();

                                    ui.label("放大/缩小 (Zoom)");
                                    ui.label("Ctrl + [ + / - ]");
                                    ui.end_row();

                                    ui.label("重置缩放 (Reset)");
                                    ui.label("Ctrl + 0");
                                    ui.end_row();

                                    ui.label("复制截图 (Copy)");
                                    ui.label("Ctrl + C");
                                    ui.end_row();

                                    ui.label("保存截图 (Save)");
                                    ui.label("Ctrl + S");
                                    ui.end_row();

                                    ui.label("取消截图 (Cancel)");
                                    ui.label("ESC");
                                    ui.end_row();

                                    ui.label("回到欢迎界面 (Back to Home)");
                                    ui.label("Q");
                                    ui.end_row();
                                });
                        });
                    });

                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    if ui.button(" 知道了 (Got it!) ").clicked() {
                        user_clicked_close = true;
                    }
                });
                ui.add_space(5.0);
            });

        if user_clicked_close {
            self.show_help_window = false;
        } else {
            self.show_help_window = is_open;
        }
    }
}
//页面旋转
impl PdfApp {
    /// 当前页旋转，angle 单位为度，例如 90 或 -90
    fn rotate_current_page(&mut self, angle: f32) {
        // pdfium-render 默认不保存旋转角度，需要我们自己维护
        // 这里用 current_page_rotation 字段记录每页旋转角度
        // 如果还没有这个字段，可以先加：
        // rotations: Vec<f32>, // 每页旋转角度，初始 0.0
        if self.pdf_doc.is_none() {
            return;
        }

        // 初始化 rotations 向量（如果尚未初始化）
        if self.rotations.len() != self.pdf_doc.as_ref().unwrap().pages().len() as usize {
            self.rotations = vec![0.0; self.pdf_doc.as_ref().unwrap().pages().len() as usize];
        }

        // 累加旋转角度
        let idx = self.current_page;
        self.rotations[idx] = (self.rotations[idx] + angle) % 360.0;

        // 触发重新渲染
        self.texture = None;
    }
}
//收藏夹
impl PdfApp {
	fn render_favorites(&mut self, ui: &mut egui::Ui) {
		let mut to_open: Option<PathBuf> = None;
		let mut file_to_remove: Option<usize> = None;
		let mut folder_to_remove: Option<usize> = None;
		let mut move_file: Option<(usize, isize)> = None;   // (索引, 方向: -1上, 1下)
		let mut move_folder: Option<(usize, isize)> = None;

		ui.vertical_centered(|ui| {
			ui.heading("Favorites");
			ui.add_space(20.0);
			// --- 切换开关 ---
			let toggle_label = if self.config.show_full_path { 
				"Display Mode: Full Path" 
			} else { 
				"Display Mode: File Name Only"
			};
			if ui.button(toggle_label).on_hover_text("Click to toggle between full file path and name only")
				.clicked() {
				self.config.show_full_path = !self.config.show_full_path;
				self.save_all_config(); // 状态改变后立即保存
			}
			ui.add_space(10.0);
			// ----------------------

			egui::ScrollArea::vertical()
				.max_height(400.0) // Optional: Cap the height so it doesn't take the whole screen
				.auto_shrink([false, true]) 
				.show(ui, |ui| {
					// Ensure buttons stay centered inside the scroll area
					ui.vertical_centered(|ui| {
						for (idx, path) in self.config.favorite_files.iter().enumerate() {
							let label = if self.config.show_full_path {
								path.display().to_string()
							} else {
								format!("📄 {}", path.file_name().unwrap_or_default().to_string_lossy())
							};

							let response = ui.button(label);

							if !self.config.show_full_path {
								response.clone().on_hover_text(path.display().to_string());
							}

							if response.clicked() {
								to_open = Some(path.clone());
							}

							response.context_menu(|ui| {
								// 移动功能
								if idx > 0 && ui.button("⬆ Move Up").clicked() {
									move_file = Some((idx, -1));
									ui.close_kind(egui::UiKind::Menu);
								}
								if idx < self.config.favorite_files.len() - 1 && ui.button("⬇ Move Down").clicked() {
									move_file = Some((idx, 1));
									ui.close_kind(egui::UiKind::Menu);
								}

								if ui.button("🔍 Open another PDF from the Folder").clicked() {
									let folder = path.parent().unwrap_or(std::path::Path::new("."));

									// 调用文件选择器，并将初始路径设置为该 PDF 所在的目录
									if let Some(new_path) = rfd::FileDialog::new()
										.add_filter("PDF files", &["pdf", "PDF"])
											.set_directory(folder) // 关键点：设置起始目录
											.pick_file() 
									{
										to_open = Some(new_path);
									}
									ui.close_kind(egui::UiKind::Menu);
								}

								if ui.button("📂 Open Containing Folder").clicked() {
									let folder = path.parent().unwrap_or(std::path::Path::new("."));

									#[cfg(target_os = "linux")]
									std::process::Command::new("xdg-open").arg(folder).spawn().ok();

									#[cfg(target_os = "windows")]
									std::process::Command::new("explorer").arg(folder).spawn().ok();

									ui.close_kind(egui::UiKind::Menu);
								}

								if ui.button("🗑 Remove from Favorites").clicked() {
									file_to_remove = Some(idx);
									ui.close_kind(egui::UiKind::Menu);
								}
							});
							ui.add_space(5.0);
						}
					});
				});

			ui.add_space(20.0);

			if ui.button("+ Add PDF to Favorites").clicked() {
				if let Some(path) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
					if !self.config.favorite_files.contains(&path) {
						self.config.favorite_files.push(path);
						self.save_all_config(); // 封装的保存方法
					}
				}
			}

			ui.add_space(50.0);
			ui.heading("Quick Access Folders");
			ui.add_space(20.0);

			egui::ScrollArea::vertical()
				.max_height(150.0) // 固定目录区高度
				.id_salt("folder_scroll") // 必须有独立的 ID
				.auto_shrink([false, true]) 
				.show(ui, |ui| {
					ui.vertical_centered(|ui| {
						for (idx, path) in self.config.favorite_folders.iter().enumerate() {
								let label = format!("📁 {}", path.display());
								let response = ui.button(label);

								if response.clicked() {
									if let Some(new_path) = rfd::FileDialog::new()
										.add_filter("PDF", &["pdf", "PDF"])
											.set_directory(path)
											.pick_file() 
									{
										to_open = Some(new_path);
									}
								}

								response.context_menu(|ui| {
									if idx > 0 && ui.button("⬆ Move Up").clicked() {
										move_folder = Some((idx, -1));
										ui.close_kind(egui::UiKind::Menu);
									}
									if idx < self.config.favorite_folders.len() - 1 && ui.button("⬇ Move Down").clicked() {
										move_folder = Some((idx, 1));
										ui.close_kind(egui::UiKind::Menu);
									}
									if ui.button("🗑 Remove Folder").clicked() {
										folder_to_remove = Some(idx);
										ui.close_kind(egui::UiKind::Menu);
									}
								});
								ui.add_space(4.0);
						}
					});
				});

			ui.add_space(20.0);
			if ui.button("+ Add Folder to Quick Access Folders").clicked() {
				if let Some(path) = rfd::FileDialog::new().pick_folder() {
					if !self.config.favorite_folders.contains(&path) {
						self.config.favorite_folders.push(path);
						self.save_all_config();
					}
				}
			}

		});

		if let Some(path) = to_open {
			self.load_pdf_path(path);
		}

		// 处理文件操作
        if let Some(idx) = file_to_remove {
            self.config.favorite_files.remove(idx);
            self.save_all_config();
        }
        if let Some((idx, dir)) = move_file {
            let target = if dir == -1 { idx - 1 } else { idx + 1 };
            self.config.favorite_files.swap(idx, target);
            self.save_all_config();
        }

        // 处理文件夹操作
        if let Some(idx) = folder_to_remove {
            self.config.favorite_folders.remove(idx);
            self.save_all_config();
        }
        if let Some((idx, dir)) = move_folder {
            let target = if dir == -1 { idx - 1 } else { idx + 1 };
            self.config.favorite_folders.swap(idx, target);
            self.save_all_config();
        }
	}
// 统一保存
    fn save_all_config(&self) {
        //save_config(&AppConfig {
        //    favorite_files: self.favorite_files.clone(),
        //    favorite_folders: self.favorite_folders.clone(),
				//		show_full_path: self.show_full_path,
				//		editor_command: self.editor_command.clone(),
        //});
				save_config(&self.config);
    }
}
//加载pdf
impl PdfApp {
	fn load_pdf_path(&mut self, path: std::path::PathBuf) -> bool {
		let path_str = path.to_string_lossy().to_string();

		// 尝试加载文档
		match self.pdfium.load_pdf_from_file(&path_str, None) {
			Ok(doc) => {
				// 判断是“自动重载”还是“打开新文件”
				let is_same_file = self.pdf_path.as_ref()
					.map_or(false, |p| p.to_string_lossy() == path_str);

				// --- 逻辑分支 A: 只有在打开【全新】文件时执行的逻辑 ---
				if !is_same_file {
					// 记录旧文件的位置到历史记录
					self.record_current_position();
					// 当加载新 PDF 时，清空之前的擦除记录
					//self.eraser_strokes.clear();
					//self.handwriting_strokes.clear();
					self.annotations.clear();
					self.current_pen_stroke.clear();
					self.current_eraser_stroke.clear();

					// 记忆新文件的文件夹
					if let Some(parent) = path.parent() {
						self.last_opened_dir = Some(parent.to_path_buf());
					}
					// 记忆文件名
					if let Some(os_str) = path.file_stem() {
						self.pdf_name = Some(os_str.to_string_lossy().to_string());
					}

					// 从历史记录恢复页码
					self.current_page = *self.history.last_pages.get(&path_str).unwrap_or(&0);

					// 启动新文件的文件监控（针对全新打开的文件）
					if let Ok(full_path) = path.canonicalize() {
						self.setup_watcher(full_path);
					}
				}


				// 更新文档和路径
				self.pdf_doc = Some(doc);
				self.pdf_path = Some(path.clone()); 

				// 初始化旋转和纹理
				let page_count = self.pdf_doc.as_ref().unwrap().pages().len() as usize;
				if self.current_page >= page_count {
					self.current_page = page_count.saturating_sub(1);
				}
				self.rotations = vec![0.0; page_count];
				self.texture = None;

				if let Some(doc_ref) = &self.pdf_doc {
					if let Ok(page) = doc_ref.pages().get(self.current_page as u16) {
						self.target_dpi = suggest_page_dpi(&page);
					}
				}

				true // 返回加载成功
			}
			Err(_e) => {
				false 
			}
		}
	}
}
//检测pdf更新
impl PdfApp {
	fn setup_watcher(&mut self, path: PathBuf) {
		let needs_reload = self.needs_reload.clone();

		// 创建监听器
		let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
			if let Ok(event) = res {
				// 只有文件内容变化时才标记刷新
				if event.kind.is_modify() {
					if let Ok(mut reload) = needs_reload.lock() {
						*reload = true;
					}
				}
			}
		}).ok();

		if let Some(ref mut w) = watcher {
			let _ = w.watch(&path, RecursiveMode::NonRecursive);
		}

		self.watcher = watcher;
	}
}
//默认截图dpi
fn suggest_page_dpi(page: &pdfium_render::prelude::PdfPage) -> f32 {
    let objects = page.objects();
    let p_w = page.width().value;
    let p_h = page.height().value;
    let page_area = p_w * p_h;

    if page_area <= 0.0 { return 300.0; }

    let mut score = 0i32;

    // 1. 文字检测 (LaTeX 首页文字虽少，但只要有，就是矢量特征)
    let char_count = page.text().map(|t| t.all().trim().len()).unwrap_or(0);
    if char_count > 10 {
        score += 50; // 有正式文字，极大概率是矢量 PDF
    }
    if char_count > 200 {
        score += 100; // 文字多，铁定是矢量/排版 PDF
    }

    // 2. 对象类型深度扫描
    let mut has_full_page_img = false;
    let mut path_count = 0;
    let mut img_count = 0;

    for obj in objects.iter() {
        match obj.object_type() {
            PdfPageObjectType::Text => score += 5, // 每一个文本对象都是强矢量证据
            PdfPageObjectType::Path => {
                path_count += 1;
                // LaTeX 的公式、线条、边框全是 Path
                if path_count > 20 { score += 20; } 
            },
            PdfPageObjectType::Image => {
                img_count += 1;
                if let Ok(bounds) = obj.bounds() {
                    let w = (bounds.right().value - bounds.left().value).abs();
                    let h = (bounds.top().value - bounds.bottom().value).abs();
                    // 只有几乎填满全屏的图片才扣分（扫描件特征）
                    if (w * h) / page_area > 0.9 {
                        has_full_page_img = true;
                    }
                }
            },
            _ => {}
        }
    }

    // 3. 最终判定逻辑
    // 扫描件判定条件：有巨型图片 且 (文字极少 且 矢量路径极少)
    if has_full_page_img && char_count < 20 && path_count < 10 {
        return 300.0;
    }

    // LaTeX 首页判定：即使文字少，但如果有 Path (Logo/线条) 或者 Image 很多且不是全屏
    if score > 40 || (img_count > 0 && !has_full_page_img) {
        return 600.0; 
    }

    300.0
}
//Reset
impl PdfApp {
	fn unload_pdf(&mut self) {
		self.record_current_position();
		self.pdf_doc = None;
		self.pdf_path = None;
		self.texture = None;
		self.current_page = 0;
		self.link_cache.clear();
		self.zoom_factor = 1.0;
		self.last_cropped_image = None;
		self.cropped_tex = None;
		// ... 其他需要重置的状态
	}
}
//PDF页面渲染
impl PdfApp {
	fn render_pdf_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
		// ---------- 1. 安全检查 ----------
		let Some(doc) = self.pdf_doc.as_ref() else { return; };
		let total_pages_count = doc.pages().len() as usize;
		let Ok(page) = doc.pages().get(self.current_page as u16) else { return; };

		// ---------- 2. PDF 页面原始尺寸 ----------
		let native_width = page.width().value;
		let native_height = page.height().value;

		// ---------- 3. 页面旋转 ----------
		let angle = self.rotations.get(self.current_page).copied().unwrap_or(0.0);
		let is_sideways = ((angle / 90.0).round() as i32).abs() % 2 != 0;
		let aspect_ratio = if is_sideways {
			native_height / native_width
		} else {
			native_width / native_height
		};

		// ---------- 4. DPI ----------
		let ppp = ctx.pixels_per_point();

		// ---------- 5. 屏幕像素计算 ----------
		let available_rect = ui.available_rect_before_wrap();
		let base_width = available_rect.width();

		let ideal_screen_px = base_width * self.zoom_factor * ppp;
		let screen_px_width = ideal_screen_px.round();
		let screen_px_height = (screen_px_width / aspect_ratio).round();

		let display_width = screen_px_width / ppp;
		let display_height = screen_px_height / ppp;
		let display_size = egui::vec2(display_width, display_height);

		// ---------- 6. Oversample ----------
		let mut target_px_width = (screen_px_width * self.view_scale).round() as u32;
		const MAX_TEXTURE_SIZE: u32 = 8192;
		target_px_width = target_px_width.min(MAX_TEXTURE_SIZE);

		// ---------- 7. 是否需要重新渲染 ----------
		let needs_re_render = self.texture.as_ref().map_or(true, |tex| {
			let current_tex_width = tex.size()[0] as u32;
			let page_changed = self.last_rendered_page != self.current_page;
			let angle_changed = (self.last_rendered_angle - angle).abs() > 0.1;
			let width_diff = (current_tex_width as i32 - target_px_width as i32).abs();
			let size_changed = width_diff > 3;
			page_changed || angle_changed || size_changed
		});

		// ---------- 9. 重新渲染 ----------
		if needs_re_render && target_px_width > 0 {
			let rotation = match ((angle / 90.0).round() as i32) % 4 {
				1 | -3 => PdfPageRenderRotation::Degrees90,
				2 | -2 => PdfPageRenderRotation::Degrees180,
				3 | -1 => PdfPageRenderRotation::Degrees270,
				_ => PdfPageRenderRotation::None,
			};

			let render_config = PdfRenderConfig::new()
				.set_target_width(target_px_width as i32)
				.rotate(rotation, true);

			if let Ok(bitmap) = page.render_with_config(&render_config) {
				let image = bitmap.as_image().to_rgba8();
				let color_image = egui::ColorImage::from_rgba_unmultiplied(
					[bitmap.width() as usize, bitmap.height() as usize],
					image.as_raw(),
				);

				self.texture = Some(ctx.load_texture(
						"pdf_page",
						color_image,
						egui::TextureOptions::LINEAR,
				));

				self.last_rendered_page = self.current_page;
				self.last_rendered_angle = angle;
			}
		}

		// ---------- 11. 绘制 ----------
		self.paint_pdf_surface(
			ui, 
			ctx, 
			&page, 
			display_size, 
			(native_width, native_height), 
			angle, 
			total_pages_count
		);
	}
}
impl PdfApp {
	fn paint_pdf_surface(
		&mut self,
		ui: &mut egui::Ui,
		ctx: &egui::Context,
		page: &pdfium_render::prelude::PdfPage,
		display_size: egui::Vec2,
		native_size: (f32, f32), // (width, height)
		angle: f32,
		total_pages: usize,
	) {
		let (native_width, native_height) = native_size;

		let scroll_source = if self.is_edit_mode {
			egui::scroll_area::ScrollSource {
				scroll_bar: true,
				drag: false, 
				mouse_wheel: true,
			}
		} else {
			egui::scroll_area::ScrollSource::ALL
		};

		egui::ScrollArea::both()
			.auto_shrink([false; 2])
			.scroll_source(scroll_source)
			.show(ui, |ui| {
				if self.scroll_delta != egui::Vec2::ZERO {
					ui.scroll_with_delta(self.scroll_delta);
					self.scroll_delta = egui::Vec2::ZERO;
				}

				let available_width = ui.available_width();
				let is_overflowing = display_size.x > available_width;

				// 1. 偏移与缩放检测逻辑 
				let x_offset = if !is_overflowing { (available_width - display_size.x) / 2.0 } else { 0.0 };

				if is_overflowing && self.last_pdf_width <= available_width 
					&& self.last_pdf_width != display_size.x {
						ui.scroll_with_delta(egui::vec2(-100.0, 0.0));
				}

				let content_size = egui::vec2(available_width.max(display_size.x), display_size.y);
				let (outer_rect, _) = ui.allocate_at_least(content_size, egui::Sense::hover());

				let pdf_rect = egui::Rect::from_min_size(
					egui::pos2(outer_rect.min.x + x_offset, outer_rect.min.y),
					display_size
				);

				// 2. 像素对齐
				let ppp = ui.ctx().pixels_per_point();
				let aligned_rect = egui::Rect::from_min_size(
					egui::pos2((pdf_rect.min.x * ppp).round() / ppp, (pdf_rect.min.y * ppp).round() / ppp),
					display_size
				);

				self.last_pdf_width = aligned_rect.width();
				self.last_pdf_height = aligned_rect.height();

				let painter = ui.painter_at(aligned_rect);
				if let Some(tex) = &self.texture {
					painter.image(tex.id(), aligned_rect,
					egui::Rect::from_min_max(egui::pos2(0.0, 0.0),
					egui::pos2(1.0, 1.0)), 
					egui::Color32::WHITE);
				}

				self.paint_static_annotations(ui, aligned_rect);

				if self.is_edit_mode {
					self.handle_edit_mode(ui, aligned_rect);
				} else {

					// 3. 交互响应
					//let mut response = ui.interact(aligned_rect, 
					//	ui.id().with("pdf_surf"), 
					//	egui::Sense::click_and_drag());
					//response.rect = aligned_rect;
					let response = ui.interact(
						aligned_rect, 
						ui.id().with("pdf_surf"), 
						egui::Sense::click_and_drag()
					);

					// --- 业务集成 ---
					if response.clicked() && ui.input(|i| i.modifiers.ctrl) {
						if let Some(pos) = response.interact_pointer_pos() { self.sync_to_latex(pos, aligned_rect); }
					}

					// --- 更新链接缓存 ---
					let page_changed = self.last_link_page != Some(self.current_page);
					let rect_changed = if let Some(last) = self.last_link_rect {
						(last.min.x - aligned_rect.min.x).abs() > 0.1 || (last.min.y - aligned_rect.min.y).abs() > 0.1
					} else { true };

					if page_changed || rect_changed {
						self.update_link_cache(&page, aligned_rect, native_width, native_height);
						self.last_link_page = Some(self.current_page);
						self.last_link_rect = Some(aligned_rect);
					}


					if angle == 0.0 && self.handle_link_interaction(ctx, &response, total_pages) {
						ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
					}
					self.handle_selection_interaction(&response, &painter, ctx, page);
				}
			});
	}
}
impl PdfApp {
	fn handle_edit_mode(&mut self, ui: &mut egui::Ui, aligned_rect: egui::Rect) {
		let resp = ui.interact(
			aligned_rect,
			ui.id().with("edit_layer"),
			egui::Sense::drag(),
		);

		// 当前鼠标位置（统一转换）
		let pointer_pos = resp.interact_pointer_pos();

		// =========================================================
		// 1. 处理输入（只负责“收集数据”，不负责绘制）
		// =========================================================
		if let Some(pos) = pointer_pos {
			let p = self.to_normalized(pos, aligned_rect);

			match self.tool_mode {
				ToolMode::Pen => {
					let now = ui.input(|i| i.time) as f32;
					let p = self.to_normalized(pos, aligned_rect);
					let hardware_force = ui.input(|i| {
						i.events.iter().find_map(|e| {
							if let egui::Event::Touch { force, .. } = e {
								*force 
							} else {
								None
							}
						})
					});

					if resp.drag_started() {
						let now = ui.input(|i| i.time) as f32;
						self.last_time_stamp = now; // 初始化时间
						let start_pressure = hardware_force.unwrap_or(0.5);
            self.last_pressure = start_pressure;
            self.current_pen_stroke = vec![StrokePoint { 
                pos: p, 
                width: self.pen_size * start_pressure 
            }];
					} else if resp.dragged() {
						if let Some(last_point) = self.current_pen_stroke.last() {
							// 计算位移和速度 (使用像素单位更准确)
							let dist = last_point.pos.distance(p) * aligned_rect.width();
							let dt = (now - self.last_time_stamp).max(0.001);
							let speed = dist / dt;
							self.last_time_stamp = now;

							// 压力模型
							let target_pressure = if let Some(force) = hardware_force {
								force
							} else {
								//1.0 - (speed / 1200.0).min(0.6)
								0.8 + (speed / 2000.0).min(0.4)
							};

							// 低通滤波平滑压力，消除手部抖动带来的粗细突变
							let alpha = 0.4; // 越小越平滑，越大响应越快
							let smooth_pressure = self.last_pressure * (1.0 - alpha) + target_pressure * alpha;
							self.last_pressure = smooth_pressure;

							if last_point.pos != p {
								self.current_pen_stroke.push(StrokePoint {
									pos: p,
									width: self.pen_size * smooth_pressure,
								});
							}
						}
					}
				}

				ToolMode::Eraser => {
					if resp.drag_started() {
						self.current_eraser_stroke.clear();
						self.current_eraser_stroke.push(p);
					} else if resp.dragged() {
						if self.current_eraser_stroke.last() != Some(&p) {
							self.current_eraser_stroke.push(p);
						}
					}
				}
			}
		}

		// =========================================================
		// 2. 收尾（保存数据）
		// =========================================================
		if resp.drag_stopped() {
			match self.tool_mode {
				ToolMode::Pen => {
					if !self.current_pen_stroke.is_empty() {
						let stroke = Stroke {
							points: self.current_pen_stroke.clone(),
							color: self.pen_color,
						};
						self.annotations
							.entry(self.current_page)
							.or_default()
							.push(Annotation::Pen(stroke)); // 统一推入
					}
					self.current_pen_stroke.clear();
				}
				ToolMode::Eraser => {
					if !self.current_eraser_stroke.is_empty() {
						let stroke = EraseStroke {
							points: self.current_eraser_stroke.clone(),
							brush_size: self.eraser_size,
							color: self.eraser_color,
						};
						self.annotations
							.entry(self.current_page)
							.or_default()
							.push(Annotation::Eraser(stroke)); // 统一推入
					}
					self.current_eraser_stroke.clear();
				}
			}
		}

		// =========================================================
		// 3. 渲染（只负责画，不负责改数据）
		// =========================================================
		let painter = ui.painter().with_clip_rect(aligned_rect);

		// -------- 当前正在画的 --------
		if self.current_pen_stroke.len() >= 2 {
			//self.draw_pressure_segments(&painter, &self.current_pen_stroke, aligned_rect, self.pen_color);
			self.draw_pressure_segments(&painter, &self.current_pen_stroke, aligned_rect, self.pen_color);
		}
		if self.current_eraser_stroke.len() >= 2 {
			let pts: Vec<egui::Pos2> = self.current_eraser_stroke.iter()
				.map(|p| self.from_normalized(*p, aligned_rect))
				.collect();
				painter.add(egui::Shape::line(pts, egui::Stroke::new(self.eraser_size, self.eraser_color)));
		}

		// =========================================================
		// 4. 光标
		// =========================================================
		self.draw_tool_cursor(ui, aligned_rect);
	}
fn draw_pressure_segments(
    &self,
    painter: &egui::Painter,
    stroke: &[StrokePoint],
    rect: egui::Rect,
    color: egui::Color32,
) {
    if stroke.len() < 2 { return; }
    
    // 转换为屏幕坐标和宽度
    let pts: Vec<(egui::Pos2, f32)> = stroke.iter()
        .map(|p| (self.from_normalized(p.pos, rect), p.width))
        .collect();

    if pts.len() < 3 {
        painter.line_segment(
            [pts[0].0, pts[1].0], 
            egui::Stroke::new(pts[1].1, color)
        );
        return;
    }

    for i in 0..pts.len() - 2 {
        let p0 = pts[i];
        let p1 = pts[i+1];
        let p2 = pts[i+2];

        let mid_start = p0.0.lerp(p1.0, 0.5);
        let mid_end = p1.0.lerp(p2.0, 0.5);

        let steps = 5;
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let t_next = (s + 1) as f32 / steps as f32;

            // 二次贝塞尔插值
            let pos_curr = mid_start.lerp(p1.0, t).lerp(p1.0.lerp(mid_end, t), t);
            let pos_next = mid_start.lerp(p1.0, t_next).lerp(p1.0.lerp(mid_end, t_next), t_next);

            // 手动实现 f32 的 lerp: start + (end - start) * t
            let current_width = p1.1 + (p2.1 - p1.1) * t;

            painter.line_segment(
                [pos_curr, pos_next],
                egui::Stroke::new(current_width, color)
            );
        }
    }
}
}
impl PdfApp {
	//fn paint_static_annotations(&self, ui: &mut egui::Ui, aligned_rect: egui::Rect) {
	//	let painter = ui.painter().with_clip_rect(aligned_rect);

	//	// 1. 绘制已保存的笔迹 (Handwriting layer)
	//	if let Some(strokes) = self.handwriting_strokes.get(&self.current_page) {
	//		for stroke in strokes {
	//			if stroke.points.len() < 2 { continue; }
	//			self.draw_pressure_segments(&painter, &stroke.points, aligned_rect, stroke.color);
	//		}
	//	}

	//	// 2. 绘制已保存的橡皮擦/遮盖 (Eraser layer)
	//	if let Some(strokes) = self.eraser_strokes.get(&self.current_page) {
	//		for stroke in strokes {
	//			if stroke.points.len() < 2 { continue; }

	//			let pts: Vec<egui::Pos2> = stroke.points.iter()
	//				.map(|p| self.from_normalized(*p, aligned_rect))
	//				.collect();

	//			painter.add(egui::Shape::line(
	//					pts,
	//					egui::Stroke::new(stroke.brush_size, stroke.color),
	//			));
	//		}
	//	}
	//}
	fn paint_static_annotations(&self, ui: &mut egui::Ui, aligned_rect: egui::Rect) {
		let painter = ui.painter().with_clip_rect(aligned_rect);

		if let Some(annos) = self.annotations.get(&self.current_page) {
			for anno in annos {
				match anno {
					Annotation::Pen(stroke) => {
						if stroke.points.len() >= 2 {
							self.draw_pressure_segments(&painter, &stroke.points, aligned_rect, stroke.color);
						}
					}
					Annotation::Eraser(stroke) => {
						if stroke.points.len() >= 2 {
							let pts: Vec<egui::Pos2> = stroke.points.iter()
								.map(|p| self.from_normalized(*p, aligned_rect))
								.collect();
							painter.add(egui::Shape::line(
									pts,
									egui::Stroke::new(stroke.brush_size, stroke.color),
							));
						}
					}
				}
			}
		}
	}
}
impl PdfApp {
fn to_normalized(&self, pos: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
    egui::pos2(
        (pos.x - rect.min.x) / rect.width(),
        (pos.y - rect.min.y) / rect.height(),
    )
}
fn from_normalized(&self, p: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
    egui::pos2(
        rect.min.x + p.x * rect.width(),
        rect.min.y + p.y * rect.height(),
    )
}
fn current_brush_color(&self) -> egui::Color32 {
    match self.tool_mode {
        ToolMode::Pen => self.pen_color,
        ToolMode::Eraser => egui::Color32::WHITE,
    }
}

fn current_brush_size(&self) -> f32 {
    match self.tool_mode {
        ToolMode::Pen => self.pen_size,
        ToolMode::Eraser => self.eraser_size,
    }
}
fn draw_tool_cursor(&self, ui: &egui::Ui, rect: egui::Rect) {
    let Some(mouse_pos) = ui.ctx().pointer_hover_pos() else {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        return;
    };

    let ui_blocking = ui.ctx().wants_pointer_input();

    if ui_blocking || !rect.contains(mouse_pos) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        return;
    }

    ui.ctx().set_cursor_icon(egui::CursorIcon::None);

    match self.tool_mode {
        ToolMode::Eraser => self.draw_eraser_cursor(ui, mouse_pos),
        ToolMode::Pen => self.draw_pen_cursor(ui, mouse_pos),
    }
}
fn draw_eraser_cursor(&self, ui: &egui::Ui, mouse_pos: egui::Pos2) {
    ui.ctx().set_cursor_icon(egui::CursorIcon::None);

    let size = self.current_brush_size();
    let rect = egui::Rect::from_center_size(mouse_pos, egui::vec2(size, size));

    ui.painter().rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.5, egui::Color32::from_black_alpha(200)),
        egui::StrokeKind::Middle,
    );
}
fn draw_pen_cursor(&self, ui: &egui::Ui, mouse_pos: egui::Pos2) {
    ui.ctx().set_cursor_icon(egui::CursorIcon::None);

    let painter = ui.painter();
    let size = self.current_brush_size();

    painter.circle_stroke(
        mouse_pos,
        size * 0.5,
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(200)),
    );

    painter.circle_filled(
        mouse_pos,
        2.0,
        self.current_brush_color(),
    );
}
}
//超链接
impl PdfApp {
	// 核心函数 A：更新缓存（仅在页面/尺寸变化时调用）
	fn update_link_cache(
		&mut self,
		page: &pdfium_render::prelude::PdfPage,
		display_rect: egui::Rect,
		native_width: f32,
		native_height: f32,
	) {
		self.link_cache.clear();
		// 关键：在这里更新对比值
		self.last_link_rect = Some(display_rect);
		// 获取页面的实际裁剪框 (CropBox)
		// 这是解决出版商 PDF 偏移的关键：找到 (0,0) 点相对于可视区域的真正位置
		let boundaries = page.boundaries();
		let (crop_left, crop_bottom) = if let Ok(crop) = boundaries.crop().as_ref() {
			(crop.bounds.left().value, crop.bounds.bottom().value)
		} else {
			(0.0, 0.0) // Fallback if no crop box is defined
		};

		let scale_x = display_rect.width() / native_width;
		let scale_y = display_rect.height() / native_height;

		for annot in page.annotations().iter() {
			if let Some(link_annot) = annot.as_link_annotation() {
				if let Ok(bounds) = annot.bounds() {
					// 减去裁剪框的偏移量
					// bounds.left() 是相对于 MediaBox 的，我们需要相对于 CropBox 的
					let adjusted_left = bounds.left().value - crop_left;
					let adjusted_right = bounds.right().value - crop_left;
					let adjusted_bottom = bounds.bottom().value - crop_bottom;
					let adjusted_top = bounds.top().value - crop_bottom;

					let x_min = display_rect.min.x + (adjusted_left * scale_x);
					let x_max = display_rect.min.x + (adjusted_right * scale_x);

					// 3. Y 轴翻转逻辑：
					// 用 native_height (可视高度) 减去相对于可视底部的 top 值
					let y_min = display_rect.min.y + (native_height - adjusted_top) * scale_y;
					let y_max = display_rect.min.y + (native_height - adjusted_bottom) * scale_y;

					let screen_rect = egui::Rect::from_min_max(
						egui::pos2(x_min, y_min),
						egui::pos2(x_max, y_max),
					);

					let mut dest_idx = None;
					let mut uri_str = None;

					if let Ok(link_data) = link_annot.link() {
						if let Some(dest) = link_data.destination() {
							dest_idx = dest.page_index().ok().map(|i| i as usize);
						} else if let Some(action) = link_data.action() {
							match action {
								pdfium_render::prelude::PdfAction::LocalDestination(goto) => {
									if let Ok(dest) = goto.destination() {
										dest_idx = dest.page_index().ok().map(|i| i as usize);
									}
								}
								pdfium_render::prelude::PdfAction::Uri(uri_action) => {
									uri_str = uri_action.uri().ok();
								}
								_ => {}
							}
						}
					}

					self.link_cache.push(CachedLink {
						rect: screen_rect,
						destination: dest_idx,
						uri: uri_str,
					});

				}
			}
		}
	}

	// 核心函数 B：处理交互（每一帧在 UI 闭包内调用）
	fn handle_link_interaction(
		&mut self,
		ctx: &egui::Context,
		response: &egui::Response,
		total_pages: usize,
	) -> bool {
		let Some(mouse_pos) = ctx.input(|i| i.pointer.hover_pos()) else { return false; };
		let mut is_over_any_link = false;

		// 遍历缓存好的链接
		for link in &self.link_cache {
			if link.rect.contains(mouse_pos) {
				is_over_any_link = true;

				if response.clicked() {
					if let Some(target_page) = link.destination {
						let new_page = target_page.min(total_pages.saturating_sub(1));
						self.current_page = new_page;
						self.texture = None; // 触发重新渲染
						ctx.request_repaint();
					} else if let Some(uri) = &link.uri {
						ctx.open_url(egui::OpenUrl::new_tab(uri));
					}
					break; // 已经处理了点击，跳出循环
				}
			}
		}
		is_over_any_link
	}
}
//矩形选择区域
impl PdfApp {
fn handle_selection_interaction(
    &mut self,
    response: &egui::Response,
    painter: &egui::Painter,
    ctx: &egui::Context,
    page: &PdfPage,
) {
    let rect_min = response.rect.min;
    let ppp = ctx.pixels_per_point();

    // 1. 获取物理按下的起始点（解决你提到的方向性偏移问题）
    // ctx.input(|i| i.pointer.press_origin()) 拿到的点是点击那一刻的位置，
    // 不会受到 egui 拖拽判定阈值（几像素的移动延迟）的影响。
    let press_origin = ctx.input(|i| i.pointer.press_origin());

    // 2. 状态：拖拽开始
    if response.drag_started() {
        if let Some(origin) = press_origin {
            // 立即转换为相对于图片左上角的坐标
            self.drag_start_local = Some((origin - rect_min).to_pos2());
        }
    }

    // 3. 状态：正在拖拽
    if response.dragged() {
        let current_global_pos = ctx.input(|i| i.pointer.interact_pos());
        
        if let (Some(start_local), Some(now_global)) = (self.drag_start_local, current_global_pos) {
            let now_local = (now_global - rect_min).to_pos2();
            
            // 构造矩形
            let local_sel = egui::Rect::from_two_pos(start_local, now_local);
            
            // 严格裁剪到图片边界内，防止超出图片范围导致裁剪失败
            let max_bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, response.rect.size());
            self.selection_rect_local = Some(local_sel.intersect(max_bounds));
        }
    }

    // 4. 状态：释放鼠标（执行裁剪）
    if response.drag_stopped() {
        if let Some(local_rect) = self.selection_rect_local {
						// A. 先执行高分辨率裁剪
            self.perform_high_res_crop(ctx, page, local_rect, response.rect.size());
						// B. 自动复制到剪贴板
            // 因为 perform_high_res_crop 已经更新了 self.last_cropped_image
						//if let Some(img) = &self.last_cropped_image {
						//	copy_image_to_clipboard(img);
						//}
        }
        // 重置状态
        self.drag_start_local = None;
        self.selection_rect_local = None;
    }

    // 5. 绘制预览层（在原图上方绘制蓝色遮罩）
    if let Some(local_sel) = self.selection_rect_local {
        // 转换回全局坐标进行绘制
        let global_sel = local_sel.translate(rect_min.to_vec2());
        
        // 绘制半透明填充
        painter.rect_filled(
            global_sel, 
            0.0, 
            egui::Color32::from_rgba_unmultiplied(0, 120, 255, 30)
        );
        
        // 绘制像素完美的描边 (Inside 模式确保边框不向外溢出)
        painter.rect_stroke(
            global_sel,
            0.0,
            egui::Stroke::new(3.0 / ppp, egui::Color32::from_rgb(0, 150, 255)),
            egui::StrokeKind::Inside
        );
    }
}
}
//截图预览
impl PdfApp {
	fn render_preview_window(&mut self, ctx: &egui::Context) {
		// 技巧：先把我们需要的数据“偷”出来，不要在 show 闭包里引用 self.cropped_tex
		let mut should_close = false;
		let mut should_copy = false;
		let mut save_format: Option<&str> = None;

		// 只在这里获取一次纹理引用
		if let Some(tex) = &self.cropped_tex {
			let tex_id = tex.id();
			let img_size = tex.size_vec2();

			egui::Window::new("High-Res-Preview")
				.id(egui::Id::new("preview_window"))
				.title_bar(false)  // 移除标题栏
				.collapsible(false)
				.resizable(true)
				.show(ctx, |ui| {
					// ... 缩放比例计算 ...
					let ratio = (700.0f32 / img_size.x).min(400.0f32 / img_size.y).min(1.0f32);
					let display_size = img_size * ratio;

					ui.vertical_centered(|ui| {
						ui.set_min_width(500.0);
						ui.add(egui::Image::new((tex_id, display_size)).sense(egui::Sense::hover()));
					});

					ui.add_space(5.0);
					ui.horizontal(|ui| {
						ui.columns(4, |cols| {
							// 第一列：Discard
							cols[0].vertical_centered(|ui| {
								if ui.button(egui::RichText::new("🗑 Discard").strong()).clicked() { 
									should_close = true;
								}
							});
							// 第二列：Copy
							cols[1].vertical_centered(|ui| {
								if ui.button(egui::RichText::new("📋 Copy").strong()).clicked() {
									should_copy = true; 
								}
							});
							
							// 第三列：Save as (合并 PNG 和 JPG)
							cols[2].vertical_centered(|ui| {
								ui.menu_button(egui::RichText::new("Save as").strong(), |ui| {
									ui.set_min_width(40.0);
									if ui.button("PNG").clicked() {
										save_format = Some("png");
										ui.close_kind(egui::UiKind::Menu);
									}
									ui.separator();
									if ui.button("JPG").clicked() {
										save_format = Some("jpg");
										ui.close_kind(egui::UiKind::Menu);
									}
								});
							});

							cols[3].vertical_centered(|ui| {
								let btn = ui.button(egui::RichText::new("OCR").strong());
								if btn.clicked() {
									self.current_latex = None; 
									self.show_ocr_window = false;
									let image_to_send = self.last_ocr_image.as_ref().or(self.last_cropped_image.as_ref());
									if let Some(img) = image_to_send {
										self.run_latex_ocr(img.clone(), ctx.clone());
									}
								}
								btn.on_hover_text("Click to get LaTeX Code");
							});

							//cols[4].vertical_centered(|ui| {
							//	ui.menu_button(egui::RichText::new("More ▼").strong(), |ui| {
							//		ui.set_min_width(120.0);

							//		// --- 快捷翻译选项 ---
							//		if ui.button("Translate to English").clicked() {
							//			self.current_latex = None; 
							//			self.show_ocr_window = false;
							//			let image_to_send = self.last_ocr_image.as_ref().or(self.last_cropped_image.as_ref());
							//			if let Some(img) = image_to_send {
							//				self.run_translate_to_lang(img.clone(), ctx.clone(), "English");
							//			}
							//			ui.close_kind(egui::UiKind::Menu);
							//		}

							//		ui.separator();

							//		if ui.button("翻译成中文").clicked() {
							//			self.current_latex = None; 
							//			self.show_ocr_window = false;
							//			let image_to_send = self.last_ocr_image.as_ref().or(self.last_cropped_image.as_ref());
							//			if let Some(img) = image_to_send {
							//				self.run_translate_to_lang(img.clone(), ctx.clone(), "Chinese");
							//			}
							//			ui.close_kind(egui::UiKind::Menu);
							//		}

							//		ui.separator();

							//		if ui.button("Translate to Spanish").clicked() {
							//			self.current_latex = None; 
							//			self.show_ocr_window = false;
							//			let image_to_send = self.last_ocr_image.as_ref().or(self.last_cropped_image.as_ref());
							//			if let Some(img) = image_to_send {
							//				self.run_translate_to_lang(img.clone(), ctx.clone(), "Spanish");
							//			}
							//			ui.close_kind(egui::UiKind::Menu);
							//		}

							//		ui.separator();

							//		// --- 特殊转换选项 ---
							//		if ui.button("Generate TikZ Code").clicked() {
							//			self.current_latex = None; 
							//			self.show_ocr_window = false;
							//			let image_to_send = self.last_ocr_image.as_ref().or(self.last_cropped_image.as_ref());
							//			if let Some(img) = image_to_send {
							//				self.run_geometry_to_tikz(img.clone(), ctx.clone());
							//			}
							//			ui.close_kind(egui::UiKind::Menu)
							//		}


							//		ui.separator();

							//		let solve_text = egui::RichText::new("Solve the Problem")
							//			.strong()
							//			.color(egui::Color32::LIGHT_BLUE);

							//		if ui.button(solve_text).clicked() {
							//			let high_res_image = self.last_ocr_image.as_ref()
							//				.or(self.last_cropped_image.as_ref())
							//				.cloned();

							//			if let Some(img) = high_res_image {
							//				self.show_ocr_window = true;
							//				self.solve_math_problem(
							//					SolveSource::Image(image::DynamicImage::ImageRgba8(img)), 
							//					ctx.clone()
							//				);
							//			}
							//			ui.close_kind(egui::UiKind::Menu);
							//		}

							//	});
							//});
						});
					});
				});
		}

		// 在 Window 闭包结束后，再去安全地修改 self 状态
		if should_close {
			self.cropped_tex = None;
			self.last_cropped_image = None;
		}
		if should_copy {
			if let Some(img) = &self.last_cropped_image {
				copy_image_to_clipboard(img);
			}
			self.cropped_tex = None;
		}
		if let Some(fmt) = save_format {
			//self.save_image_with_format(fmt);
			let img = self.last_cropped_image.clone();
			self.save_generic_image(img, "Crop", fmt);
		}
	}
}
//截图
impl PdfApp {
fn perform_high_res_crop(
    &mut self, 
    ctx: &egui::Context, 
    page: &PdfPage, 
    local_rect: egui::Rect, 
    actual_ui_size: egui::Vec2 // 虽然仍传入，但仅用于坐标映射
) {
    // 1. 设置目标 DPI (打印级清晰度通常是 300 DPI)
		let dpi = self.target_dpi;

    // 2. 获取 PDF 页面在 72 DPI 下的原始 Point 尺寸
    // 注意：这里需要区分是否旋转，因为 point_width() 返回的是未旋转的
    let angle = self.rotations.get(self.current_page).copied().unwrap_or(0.0);
    let is_sideways = ((angle / 90.0).round() as i32).abs() % 2 != 0;

    let page_point_width = if is_sideways { page.height().value } else { page.width().value };
    // let page_point_height = if is_sideways { page.width().value } else { page.height().value }; // 如果需要高度

    // 3. 计算基于 DPI 的目标像素宽度
    // 物理尺寸 (英寸) = page_point_width / 72.0
    // 目标像素 = 物理尺寸 * dpi
    let target_pixel_width = (page_point_width / 72.0 * dpi).round() as u32;

    // --- 获取当前页面的旋转角度 (保持原样) ---
    let rotation = match ((angle / 90.0).round() as i32) % 4 {
        1 | -3 => PdfPageRenderRotation::Degrees90,
        2 | -2 => PdfPageRenderRotation::Degrees180,
        3 | -1 => PdfPageRenderRotation::Degrees270,
        _ => PdfPageRenderRotation::None,
    };

    // --- 创建渲染配置 (使用新的 target_pixel_width) ---
    let render_config = PdfRenderConfig::new()
        .set_target_width(target_pixel_width.try_into().unwrap())
        .rotate(rotation, true);

    if let Ok(bitmap) = page.render_with_config(&render_config) {
        let full_image = bitmap.as_image().to_rgba8();
        
        let img_w = full_image.width() as f32;
        let img_h = full_image.height() as f32;

        // --- 坐标映射区 (保持原样，因为比例关系未变) ---
        let left_ratio = (local_rect.min.x / actual_ui_size.x).clamp(0.0, 1.0);
        let top_ratio = (local_rect.min.y / actual_ui_size.y).clamp(0.0, 1.0);
        let width_ratio = (local_rect.width() / actual_ui_size.x).clamp(0.0, 1.0);
        let height_ratio = (local_rect.height() / actual_ui_size.y).clamp(0.0, 1.0);

        // 计算裁剪像素 (基于新渲染的高分大图)
        let mut crop_x = (left_ratio * img_w).floor() as u32;
        let mut crop_y = (top_ratio * img_h).floor() as u32;
        let mut crop_w = (width_ratio * img_w).ceil() as u32;
        let mut crop_h = (height_ratio * img_h).ceil() as u32;

        // --- 崩溃防御区 (保持原样) ---
        crop_x = crop_x.min(full_image.width().saturating_sub(1));
        crop_y = crop_y.min(full_image.height().saturating_sub(1));
        crop_w = crop_w.min(full_image.width() - crop_x);
        crop_h = crop_h.min(full_image.height() - crop_y);

        if crop_w > 0 && crop_h > 0 {
            use image::GenericImageView;
            if crop_x + crop_w <= full_image.width() && crop_y + crop_h <= full_image.height() {
							// --- 1. 获取原始高分裁剪图 (保持现有逻辑，用于显示和保存) ---
                let cropped_sub_image = full_image.view(crop_x, crop_y, crop_w, crop_h).to_image();
                self.last_cropped_image = Some(cropped_sub_image.clone());

								// --- 2. 新增：生成 200 DPI 的 OCR 专用图 ---
								// 计算缩放比例：从 target_dpi 降到 200
								let ocr_dpi = 200.0;
								let scale_factor = ocr_dpi / self.target_dpi; // 比如 200 / 600 = 0.33

								if scale_factor < 1.0 {
									let ocr_w = (cropped_sub_image.width() as f32 * scale_factor) as u32;
									let ocr_h = (cropped_sub_image.height() as f32 * scale_factor) as u32;

									// 使用 Triangle (线性插值) 缩放，速度快且适合 OCR
									let ocr_image = image::imageops::resize(
										&cropped_sub_image, 
										ocr_w.max(1), 
										ocr_h.max(1), 
										image::imageops::FilterType::Triangle 
									);
									self.last_ocr_image = Some(ocr_image);
								} else {
									// 如果设置的 target_dpi 本来就小于等于 200，就直接复用
									self.last_ocr_image = Some(cropped_sub_image.clone());
								}

								// --- 3. 纹理更新 (用于 UI 预览，依然用高分图) ---
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [cropped_sub_image.width() as usize, cropped_sub_image.height() as usize],
                    cropped_sub_image.as_raw(),
                );

                self.cropped_tex = Some(ctx.load_texture(
                    "crop_result",
                    color_image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
    }
}
}
//保存图片
impl PdfApp {
    fn save_generic_image(&mut self, img_to_save: Option<image::RgbaImage>, prefix: &str, extension: &str) {
        if let Some(img_data) = img_to_save {
            let base_name = self.pdf_name.as_deref().unwrap_or("Document");
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
            // 根据传入的 prefix 生成文件名，例如 Document_Preview_20260324.png
            let default_name = format!("{}_{}_{}.{}", base_name, prefix, timestamp, extension);

            let file = rfd::FileDialog::new()
                .set_file_name(&default_name)
                .add_filter(extension, &[extension])
                .save_file();

            if let Some(path) = file {
                if extension == "jpg" || extension == "jpeg" {
                    // 1. 创建白色背景并去透明化
                    let mut rgb_img = image::ImageBuffer::new(img_data.width(), img_data.height());
                    for pixel in rgb_img.pixels_mut() {
                        *pixel = image::Rgb([255, 255, 255]);
                    }

                    for (x, y, rgba) in img_data.enumerate_pixels() {
                        if rgba[3] > 0 { 
                            let rgb = rgb_img.get_pixel_mut(x, y);
                            *rgb = image::Rgb([rgba[0], rgba[1], rgba[2]]);
                        }
                    }

                    // 2. 保存为 JPG
                    match std::fs::File::create(&path) {
                        Ok(file) => {
                            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 90);
                            if let Err(e) = encoder.encode_image(&rgb_img) {
                                eprintln!("Failed to encode JPG: {}", e);
                            }
                        }
                        Err(e) => eprintln!("Failed to create file: {}", e),
                    }
                } else {
                    // 3. 保存为 PNG
                    if let Err(e) = img_data.save(&path) {
                        eprintln!("Failed to save PNG: {}", e);
                    }
                }
            }
        }
    }
}
//复制图片
fn copy_image_to_clipboard(img: &image::RgbaImage) {
	let mut clipboard = match arboard::Clipboard::new() {
		Ok(c) => c,
		Err(e) => {
			eprintln!("Failed to init clipboard: {}", e);
			return;
		}
	};

	// 将 image 库的 RgbaImage 转换为 arboard 的 ImageData
	let image_data = arboard::ImageData {
		width: img.width() as usize,
		height: img.height() as usize,
		bytes: std::borrow::Cow::Borrowed(img.as_raw()),
	};

	if let Err(e) = clipboard.set_image(image_data) {
		eprintln!("Failed to copy image: {}", e);
	} 
}
//提取页面
impl PdfApp {
	fn render_export_window(&mut self, ctx: &egui::Context) {
		let mut open = self.show_export_window;

		egui::Window::new("📤 Export Pages")
			.open(&mut open) 
			.resizable(false)
			.collapsible(false)
			.anchor(egui::Align2::RIGHT_TOP, [-20.0, 40.0])
			.show(ctx, |ui| {
				ui.set_width(260.0);

				ui.vertical_centered(|ui| {
					ui.add_space(5.0);
					ui.heading("Extract Current Page");
					ui.add_space(5.0);

					ui.vertical_centered_justified(|ui| {
						if ui.button("Export current page as PNG").clicked() {
							self.export_current_page_with_format("png");
							self.show_export_window = false; 
						}
						ui.add_space(5.0);
						if ui.button("Export current page as JPG").clicked() {
							self.export_current_page_with_format("jpg");
							self.show_export_window = false;
						}
						ui.add_space(5.0);
						if ui.button("Extract current page as PDF").clicked() {
							self.extract_current_page_pdf();
							self.show_export_window = false;
						}
					});
				});

				ui.add_space(10.0);
				ui.separator();
				ui.add_space(10.0);

				ui.vertical_centered(|ui| {
					ui.heading("Extract Pages Range");
					ui.add_space(5.0);

					ui.horizontal(|ui| {
						ui.label("Pages:");
						ui.text_edit_singleline(&mut self.export_range_text);
					});
					ui.weak("Example: 1-5, 8, 12");

					ui.add_space(8.0);

					ui.vertical_centered_justified(|ui| {
						if ui.button("Export Pages Range as PNGs").clicked() {
							self.export_pages_bulk("png");
							self.show_export_window = false;
						}
						ui.add_space(5.0);
						if ui.button("Export Pages Range as JPGs").clicked() {
							self.export_pages_bulk("jpg");
							self.show_export_window = false;
						}
						ui.add_space(5.0);
						if ui.button("Merge Pages Range to PDF").clicked() {
							self.extract_pages_as_single_pdf();
							self.show_export_window = false;
						}
					});
				});
				ui.add_space(10.0);
			});

		if !open {
			self.show_export_window = false;
		}
	}
	fn save_image_to_path(&self, img_data: image::DynamicImage, path: &std::path::Path, extension: &str) {
		let ext_lower = extension.to_lowercase();

		if ext_lower == "jpg" || ext_lower == "jpeg" {
			let mut rgb_img = image::ImageBuffer::new(img_data.width(), img_data.height());
			for (x, y, pixel) in rgb_img.enumerate_pixels_mut() {
				// 注意这里需要 use image::GenericImageView;
				let rgba = image::GenericImageView::get_pixel(&img_data, x, y);
				if rgba[3] == 0 {
					*pixel = image::Rgb([255, 255, 255]);
				} else {
					*pixel = image::Rgb([rgba[0], rgba[1], rgba[2]]);
				}
			}
			if let Ok(file) = std::fs::File::create(path) {
				let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 80);
				let _ = encoder.encode_image(&rgb_img);
			}
		} else {
			let _ = img_data.save(path);
		}
	}
	fn export_current_page_with_format(&mut self, extension: &str) {
    let Some(doc) = &self.pdf_doc else { return; };
    let Ok(page) = doc.pages().get(self.current_page as u16) else { return; };

    let page_width_pt = page.width().value;
    let target_width = (page_width_pt / 72.0 * self.target_dpi) as i32;

    let config = PdfRenderConfig::new().set_target_width(target_width);
    let render_res = page.render_with_config(&config);

    if let Ok(render) = render_res {
        let img_data_rgba = render.as_image().to_rgba8();
        let mut img_data = image::DynamicImage::ImageRgba8(img_data_rgba);

        if self.is_edit_mode {
            // 关键：按统一顺序渲染到图片上
            if let Some(annos) = self.annotations.get(&self.current_page) {
                let (img_w, img_h) = img_data.dimensions();
                
                for anno in annos {
                    match anno {
                        Annotation::Pen(stroke) => {
                            self.draw_pen_on_image(&mut img_data, stroke, img_w, img_h);
                        }
                        Annotation::Eraser(stroke) => {
                            self.draw_eraser_on_image(&mut img_data, stroke, img_w, img_h);
                        }
                    }
                }
            }
        }

        // 保存文件部分
        let base_name = self.pdf_name.as_deref().unwrap_or("Document");
        let default_name = format!("{}_Page_{}.{}", base_name, self.current_page + 1, extension);

        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter(extension, &[extension])
            .save_file() 
        {
            self.save_image_to_path(img_data, &path, extension);
        }
    }
}
	fn extract_current_page_pdf(&mut self) {
		// 1. 安全检查：确保 PDF 已加载
		let Some(doc) = &self.pdf_doc else { return; };

		// 2. 准备与图片导出一致的默认文件名
		let base_name = self.pdf_name.as_deref().unwrap_or("Document");
		let page_num = self.current_page + 1;
		let default_name = format!("{}_Page_{}.pdf", base_name, page_num);

		// 3. 弹出保存对话框，应用默认文件名和过滤器
		if let Some(path) = rfd::FileDialog::new()
			.set_file_name(&default_name)
				.add_filter("PDF document", &["pdf"])
				.save_file()
		{
			// 4. 创建新文档并复制页面
			let mut new_doc = self.pdfium.create_new_pdf().unwrap();

			// 将原文档的当前页复制到新文档的第 0 页位置
			if let Err(e) = new_doc.pages_mut().copy_page_from_document(
				doc,
				self.current_page as u16,
				0,
			) {
				eprintln!("Failed to copy page: {}", e);
				return;
			}

			if self.is_edit_mode {
				if let Ok(mut page) = new_doc.pages().get(0) {
					if let Some(metrics) = self.get_page_metrics_tuple(&page) {
						if let Some(annos) = self.annotations.get(&self.current_page) {
							for anno in annos {
								match anno {
									Annotation::Pen(stroke) => {
										let _ = self.apply_single_pen_stroke(&new_doc, &mut page, stroke, metrics);
									}
									Annotation::Eraser(stroke) => {
										let _ = self.apply_single_eraser(&new_doc, &mut page, stroke, metrics);
									}
								}
							}
						}
					}
				}
			}

			// 5. 保存到指定路径
			if let Err(e) = new_doc.save_to_file(&path) {
				eprintln!("Failed to save PDF: {}", e);
			} 
		}
	}
	fn parse_page_range(&self, input: &str, max_pages: u16) -> Vec<u16> {
		let mut pages = Vec::new();

		// 1. 预处理：将中文逗号、分号替换为英文的，然后过滤掉除数字、英文分隔符外的多余字符
		let cleaned_input: String = input
			.replace('，', ",") // 中文逗号
			.replace('；', ",") // 中文分号
			.replace(';', ",")  // 统一把英文分号也转成逗号，方便后续切分
			.replace('—', "-")  // 中文破折号（有时会被当成范围符）
			.chars()
			.filter(|c| c.is_ascii_digit() || *c == ',' || *c == '-')
			.collect();

		// 2. 按照逗号切分
		for part in cleaned_input.split(',') {
			let part = part.trim();
			if part.is_empty() { continue; }

			if part.contains('-') {
				// 处理范围 3-5
				let range_parts: Vec<&str> = part.split('-').collect();
				if range_parts.len() >= 2 {
					let s_res = range_parts[0].parse::<u16>();
					let e_res = range_parts[1].parse::<u16>();

					if let (Ok(s), Ok(e)) = (s_res, e_res) {
						// 自动处理 10-5 情况，并限制在合法页码范围内
						let start = s.min(e).max(1);
						let end = s.max(e).min(max_pages);

						for i in start..=end {
							pages.push(i - 1);
						}
					}
				}
			} else {
				// 处理单个数字
				if let Ok(num) = part.parse::<u16>() {
					if num > 0 && num <= max_pages {
						pages.push(num - 1);
					}
				}
			}
		}

		// 3. 排序并去重
		pages.sort_unstable();
		pages.dedup();
		pages
	}
	fn export_pages_bulk(&mut self, extension: &str) {
		let Some(doc) = &self.pdf_doc else { return; };
		let total_pages = doc.pages().len();
		let target_pages = self.parse_page_range(&self.export_range_text, total_pages);

		if target_pages.is_empty() { return; }

		// 选择保存目录
		if let Some(folder_path) = rfd::FileDialog::new().pick_folder() {
			let base_name = self.pdf_name.as_deref().unwrap_or("Document");

			for &page_idx in &target_pages {
				if let Ok(page) = doc.pages().get(page_idx) {
					// 渲染逻辑 (复用之前的渲染代码)
					let page_width_pt = page.width().value;
					let target_width = (page_width_pt / 72.0 * self.target_dpi) as i32;
					if let Ok(render) = page.render_with_config(&PdfRenderConfig::new().set_target_width(target_width)) {
						let img_data = render.as_image();
						let file_name = format!("{}_Page_{}.{}", base_name, page_idx + 1, extension);
						let path = folder_path.join(file_name);

						// 这里调用你之前的保存逻辑（JPG填白，PNG直接存）
						self.save_image_to_path(img_data, &path, extension);
					}
				}
			}
		}
	}
	fn extract_pages_as_single_pdf(&mut self) {
		let Some(doc) = &self.pdf_doc else { return; };
		let Some(original_path) = &self.pdf_path else { return; };

		let total_pages = doc.pages().len();
		let target_pages = self.parse_page_range(&self.export_range_text, total_pages);

		if target_pages.is_empty() {
			return;
		}

		//let default_name = format!(
		//    "{}_Selected_Pages.pdf",
		//    self.pdf_name.as_deref().unwrap_or("Document")
		//);
		let range_desc = self.generate_range_string(&target_pages);
		let default_name = format!(
			"{}_{}.pdf",
			self.pdf_name.as_deref().unwrap_or("Document"),
			range_desc
		);

		if let Some(path) = rfd::FileDialog::new()
			.set_file_name(&default_name)
				.add_filter("PDF", &["pdf"])
				.save_file()
		{
			// 重新加载 PDF
			let new_doc = match self.pdfium.load_pdf_from_file(original_path, None) {
				Ok(doc) => doc,
				Err(e) => {
					eprintln!("Failed to reload PDF: {}", e);
					return;
				}
			};

			let total = new_doc.pages().len();

			let pages_to_keep: HashSet<_> = target_pages.iter().copied().collect();

			// 倒序删除
			for i in (0..total).rev() {
				if !pages_to_keep.contains(&i) {
					if let Ok(page) = new_doc.pages().get(i) {
						//page.delete();
						if let Err(e) = page.delete() {
							eprintln!("Failed to delete page {}: {}", i + 1, e);
						}
					}
				}
			}

			if self.is_edit_mode {
				for (new_idx, &orig_idx) in target_pages.iter().enumerate() {
					if let Ok(mut page) = new_doc.pages().get(new_idx as u16) {
						if let Some(metrics) = self.get_page_metrics_tuple(&page) {
							if let Some(annos) = self.annotations.get(&(orig_idx as usize)) {
								for anno in annos {
									match anno {
										Annotation::Pen(stroke) => {
											let _ = self.apply_single_pen_stroke(&new_doc, &mut page, stroke, metrics);
										}
										Annotation::Eraser(stroke) => {
											let _ = self.apply_single_eraser(&new_doc, &mut page, stroke, metrics);
										}
									}
								}
							}
						}
					}
				}
			}

			if let Err(e) = new_doc.save_to_file(&path) {
				eprintln!("Failed to save PDF: {}", e);
			}
		}
	}
	fn generate_range_string(&self, indices: &[u16]) -> String {
		if indices.is_empty() { return String::new(); }

		let mut result = Vec::new();
		let mut start = indices[0];
		let mut end = indices[0];

		for &next in indices.iter().skip(1) {
			if next == end + 1 {
				end = next;
			} else {
				if start == end {
					result.push(format!("{}", start + 1));
				} else {
					result.push(format!("{}-{}", start + 1, end + 1));
				}
				start = next;
				end = next;
			}
		}

		// 处理最后一组
		if start == end {
			result.push(format!("{}", start + 1));
		} else {
			result.push(format!("{}-{}", start + 1, end + 1));
		}

		result.join("_")
	}
}
impl PdfApp {
    // 渲染笔迹：带压力和颜色
    fn draw_pen_on_image(&self, img: &mut image::DynamicImage, stroke: &Stroke, w: u32, h: u32) {
        if stroke.points.len() < 2 { return; }
        
        let rgba = stroke.color.to_array(); // [r, g, b, a]
        let color = image::Rgba([rgba[0], rgba[1], rgba[2], rgba[3]]);

        // 复刻贝塞尔逻辑
        if stroke.points.len() < 3 {
            let p1 = stroke.points[0];
            let p2 = stroke.points[1];
            self.draw_line_on_image(img, p1.pos, p2.pos, p2.width, color, w, h);
        } else {
            for i in 0..stroke.points.len() - 2 {
                let p0 = stroke.points[i];
                let p1 = stroke.points[i+1];
                let p2 = stroke.points[i+2];

                let mid_start = p0.pos.lerp(p1.pos, 0.5);
                let mid_end = p1.pos.lerp(p2.pos, 0.5);

                let steps = 5;
                for s in 0..steps {
                    let t = s as f32 / steps as f32;
                    let t_next = (s + 1) as f32 / steps as f32;

                    let pos_curr = mid_start.lerp(p1.pos, t).lerp(p1.pos.lerp(mid_end, t), t);
                    let pos_next = mid_start.lerp(p1.pos, t_next).lerp(p1.pos.lerp(mid_end, t_next), t_next);
                    let current_width = p1.width + (p2.width - p1.width) * t;

                    self.draw_line_on_image(img, pos_curr, pos_next, current_width, color, w, h);
                }
            }
        }
    }

    // 渲染橡皮擦
    fn draw_eraser_on_image(&self, img: &mut image::DynamicImage, stroke: &EraseStroke, w: u32, h: u32) {
        let rgba = stroke.color.to_array();
        let color = image::Rgba([rgba[0], rgba[1], rgba[2], rgba[3]]);
        
        for window in stroke.points.windows(2) {
            self.draw_line_on_image(img, window[0], window[1], stroke.brush_size, color, w, h);
        }
    }

    // 底层绘制方法：将归一化坐标转换为像素坐标并绘制
    fn draw_line_on_image(&self, img: &mut image::DynamicImage, p1: egui::Pos2, p2: egui::Pos2, width: f32, color: image::Rgba<u8>, w: u32, h: u32) {
        let x1 = (p1.x * w as f32) as i32;
        let y1 = (p1.y * h as f32) as i32; // 注意：如果是 top-left 坐标系，这里可能不需要 1.0 - p.y，取决于你 to_normalized 的实现
        let x2 = (p2.x * w as f32) as i32;
        let y2 = (p2.y * h as f32) as i32;

        // 这里调用你原来的 draw_line，但增加颜色参数
        draw_line_with_color(img, x1, y1, x2, y2, (width / 2.0) as i32, color);
    }
}
fn draw_circle_with_color(img: &mut image::DynamicImage, cx: i32, cy: i32, radius: i32, color: image::Rgba<u8>) {
    let (w, h) = img.dimensions();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            if dx * dx + dy * dy <= radius * radius {
                let x = cx + dx;
                let y = cy + dy;
                if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                    img.put_pixel(x as u32, y as u32, color);
                }
            }
        }
    }
}
fn draw_line_with_color(
    img: &mut image::DynamicImage,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    thickness: i32,
    color: image::Rgba<u8>,
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };

    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;

    loop {
        // 在当前点画一个圆来表示线条的粗细
        // 注意：thickness 实际上是半径
        draw_circle_with_color(img, x, y, thickness, color);

        if x == x1 && y == y1 {
            break;
        }

        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

//保存修改后的pdf
impl PdfApp {
	fn save_as_pdf(&mut self, output_path: std::path::PathBuf) -> Result<(), Box<dyn std::error::Error>> {
		let doc = self.pdf_doc.as_ref().ok_or("No document loaded")?;

		for (&page_index, annos) in &self.annotations {
			let mut page = doc.pages().get(page_index as u16)?;

			// 预先获取页面度量信息
			let Some(metrics) = self.get_page_metrics_tuple(&page) else { continue; };

			// 核心：按用户操作的先后顺序写入 PDF
			for anno in annos {
				match anno {
					Annotation::Pen(stroke) => {
						self.apply_single_pen_stroke(doc, &mut page, stroke, metrics)?;
					}
					Annotation::Eraser(stroke) => {
						self.apply_single_eraser(doc, &mut page, stroke, metrics)?;
					}
				}
			}
		}

		doc.save_to_file(&output_path)?;
		Ok(())
	}

	// 辅助函数，将 metrics 转为元组方便传递
	fn get_page_metrics_tuple(&self, page: &pdfium_render::prelude::PdfPage) -> Option<(f32, f32, f32, f32, f32)> {
		let bounds = page.boundaries().crop().ok().or_else(|| page.boundaries().media().ok())?.bounds;
		let left = bounds.left().value;
		let bottom = bounds.bottom().value;
		let pw = (bounds.right().value - left).abs();
		let ph = (bounds.top().value - bottom).abs();
		let scale_factor = pw / self.last_pdf_width.max(1.0);
		Some((left, bottom, pw, ph, scale_factor))
	}
}
impl PdfApp {
	// 处理单条擦除
	fn apply_single_eraser<'a>(
		&self,
		doc: &pdfium_render::prelude::PdfDocument<'a>,
		page: &mut pdfium_render::prelude::PdfPage<'a>,
		stroke: &EraseStroke,
		metrics: (f32, f32, f32, f32, f32),
	) -> Result<(), Box<dyn std::error::Error>> {
		let (left, bottom, pw, ph, scale_factor) = metrics;

		let stroke_width = PdfPoints::new(stroke.brush_size * scale_factor);
		let stroke_color = PdfColor::new(stroke.color.r(), stroke.color.g(), stroke.color.b(), stroke.color.a());

		let map = |p: &egui::Pos2| {
			let x = left + p.x * pw;
			let y = bottom + (1.0 - p.y) * ph;
			(PdfPoints::new(x), PdfPoints::new(y))
		};

		let (sx, sy) = map(&stroke.points[0]);
		let mut path = PdfPagePathObject::new(doc, sx, sy, Some(stroke_color), Some(stroke_width), None)?;

		for p in stroke.points.iter().skip(1) {
			let (nx, ny) = map(p);
			let _ = path.line_to(nx, ny);
		}
		page.objects_mut().add_path_object(path)?;
		Ok(())
	}

	// 处理单条笔迹 (含压力感应插值)
	fn apply_single_pen_stroke<'a>(
		&self,
		doc: &pdfium_render::prelude::PdfDocument<'a>,
		page: &mut pdfium_render::prelude::PdfPage<'a>,
		stroke: &Stroke,
		metrics: (f32, f32, f32, f32, f32),
	) -> Result<(), Box<dyn std::error::Error>> {
		let (left, bottom, pw, ph, scale_factor) = metrics;

		let map_pos = |p: egui::Pos2| {
			let x = left + p.x * pw;
			let y = bottom + (1.0 - p.y) * ph;
			(PdfPoints::new(x), PdfPoints::new(y))
		};

		let stroke_color = Some(PdfColor::new(stroke.color.r(), stroke.color.g(), stroke.color.b(), stroke.color.a()));

		if stroke.points.len() < 3 {
			let (sx, sy) = map_pos(stroke.points[0].pos);
			let (ex, ey) = map_pos(stroke.points[1].pos);
			let width = Some(PdfPoints::new(stroke.points[1].width * scale_factor));
			let mut path = PdfPagePathObject::new(doc, sx, sy, stroke_color, width, None)?;
			path.line_to(ex, ey).ok();
			page.objects_mut().add_path_object(path)?;
		} else {
			for i in 0..stroke.points.len() - 2 {
				let p0 = &stroke.points[i];
				let p1 = &stroke.points[i+1];
				let p2 = &stroke.points[i+2];

				let mid_start = p0.pos.lerp(p1.pos, 0.5);
				let mid_end = p1.pos.lerp(p2.pos, 0.5);

				let steps = 5;
				for s in 0..steps {
					let t = s as f32 / steps as f32;
					let t_next = (s + 1) as f32 / steps as f32;

					let pos_curr = mid_start.lerp(p1.pos, t).lerp(p1.pos.lerp(mid_end, t), t);
					let pos_next = mid_start.lerp(p1.pos, t_next).lerp(p1.pos.lerp(mid_end, t_next), t_next);
					let current_width = (p1.width + (p2.width - p1.width) * t) * scale_factor;

					let (sx, sy) = map_pos(pos_curr);
					let (nx, ny) = map_pos(pos_next);

					let mut path_seg = PdfPagePathObject::new(doc, sx, sy, stroke_color, Some(PdfPoints::new(current_width)), None)?;
					path_seg.line_to(nx, ny).ok();
					page.objects_mut().add_path_object(path_seg)?;
				}
			}
		}
		Ok(())
	}
}

//合并PDF
impl PdfApp {
	fn render_merge_window(&mut self, ctx: &egui::Context) {
		if !self.show_merge_window { return; }

		// 1. 定义局部变量用于同步右上角关闭按钮
		let mut open = self.show_merge_window;

		egui::Window::new("Merge Files to PDF")
			.open(&mut open)
			.resizable(true)
			.default_width(400.0)
			.show(ctx, |ui| {
				ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
				ui.vertical(|ui| {
					ui.label("Add PDF files or Images to merge them into one PDF.");

					ui.columns(2, |columns| {
						columns[0].vertical_centered(|ui| {
							if ui.button("➕ Add Files").clicked() {
								if let Some(files) = rfd::FileDialog::new()
									.add_filter("Supported Files", &["pdf", "png", "jpg", "jpeg"])
										.pick_files() 
								{
									self.merge_file_list.extend(files);
								}
							}
						});
						columns[1].vertical_centered(|ui| {
							if ui.button("🗑 Clear All").clicked() {
								self.merge_file_list.clear();
							}
						});
					});

					ui.separator();

					egui::ScrollArea::vertical()
						.max_height(300.0)
						.min_scrolled_height(100.0)
						.auto_shrink([false, false])
						.show(ui, |ui| {
						let mut to_remove = None;
						let mut move_up = None;
						let mut move_down = None;
						let len = self.merge_file_list.len();
						for (i, path) in self.merge_file_list.iter().enumerate() {
							ui.horizontal(|ui| {
								ui.add_space(5.0);
								let name = path.file_name().unwrap_or_default().to_string_lossy();
								let icon = if name.to_lowercase().ends_with(".pdf") { "📄" } else { "🖼" };
								ui.label(format!("{} {}.    {}", icon, i + 1, name));
								ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 删除按钮
                    if ui.button("❌").on_hover_text("Remove").clicked() {
                        to_remove = Some(i);
                    }

                    // 下移按钮：最后一项不可点击
                    ui.add_enabled_ui(i < len - 1, |ui| {
                        if ui.button("⏷").on_hover_text("Move Down").clicked() {
                            move_down = Some(i);
                        }
                    });

                    // 上移按钮：第一项不可点击
                    ui.add_enabled_ui(i > 0, |ui| {
                        if ui.button("⏶").on_hover_text("Move Up").clicked() {
                            move_up = Some(i);
                        }
                    });
                });
							});
						}
						if let Some(idx) = to_remove { self.merge_file_list.remove(idx); }
						if let Some(idx) = move_up { self.merge_file_list.swap(idx, idx - 1); }
						if let Some(idx) = move_down { self.merge_file_list.swap(idx, idx + 1); }
					});

					ui.separator();

					ui.vertical_centered(|ui| {
						ui.add_enabled_ui(!self.merge_file_list.is_empty(), |ui| {
							if ui.button("Generate Merged PDF").clicked() {
								self.process_merge_files();
								self.show_merge_window = false; 
							}
						});
					});
					ui.add_space(5.0);
				});
			});

		// 3. 同步状态：如果用户点了右上角的 X，open 会变 false
		if !open {
			self.show_merge_window = false;
		}
	}
	fn process_merge_files(&mut self) {
		if self.merge_file_list.is_empty() { return; }

		if let Some(save_path) = rfd::FileDialog::new()
			.set_file_name("Merged_Document.pdf")
				.add_filter("PDF", &["pdf"])
				.save_file() 
		{
			let mut new_doc = self.pdfium.create_new_pdf().unwrap();
			let mut has_content = false;


			for path in &self.merge_file_list {
				let ext = path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_lowercase();

				if ext == "pdf" {
					if let Ok(src_doc) = self.pdfium.load_pdf_from_file(path, None) {
						let page_count = src_doc.pages().len();
						if page_count > 0 {
							// --- 核心修复：提前获取长度 ---
							let destination_index = new_doc.pages().len(); 

							let _ = new_doc.pages_mut().copy_page_range_from_document(
								&src_doc, 
								0..=(page_count - 1), 
								destination_index // 使用提前存好的变量
							);
							has_content = true;
						}
					}
				} else if ["jpg", "jpeg", "png"].contains(&ext.as_str()) {
					// 1. 使用 image 库打开图片
					if let Ok(img) = image::open(path) {
						let (width_px, height_px) = (img.width(), img.height());

						// PDF 转换：Px -> Pt (f32 类型转换)
						let width_pt = printpdf::Pt(width_px as f32 * 0.75);
						let height_pt = printpdf::Pt(height_px as f32 * 0.75);

						// 2. 创建临时 PDF
						// 注意：new 方法需要 Mm 类型，我们用 .into() 自动转换
						let (doc, page1, layer1) = printpdf::PdfDocument::new(
							"temp_img", 
							width_pt.into(), 
							height_pt.into(), 
							"Layer 1"
						);

						// 3. 关键：将 DynamicImage 转换为 printpdf 的 Image
						// 我们手动创建一个 ImageXObject，这是最不容易出 trait 错误的方法
						let image_x_object = printpdf::ImageXObject {
							width: printpdf::Px(width_px as usize),
							height: printpdf::Px(height_px as usize),
							color_space: printpdf::ColorSpace::Rgb,
							bits_per_component: printpdf::ColorBits::Bit8,
							interpolate: true,
							image_data: img.to_rgb8().into_raw(),
							image_filter: None,
							clipping_bbox: None,
							smask: None,
						};

						let print_image = printpdf::Image::from(image_x_object);

						// 使用默认变换将图片放置在页面上
						print_image.add_to_layer(
							doc.get_page(page1).get_layer(layer1), 
							printpdf::ImageTransform {
								translate_x: Some(printpdf::Pt(0.0).into()),
								translate_y: Some(printpdf::Pt(0.0).into()),
								rotate: None,
								scale_x: Some(0.75), // 关键：缩放比例，对应之前的像素转换
								scale_y: Some(0.75),
								dpi: Some(72.0),
							}
						);

						// 4. 导出并交给 Pdfium 合并
						if let Ok(pdf_bytes) = doc.save_to_bytes() {
							if let Ok(temp_src_doc) = self.pdfium.load_pdf_from_byte_vec(pdf_bytes, None) {
								let destination_index = new_doc.pages().len();
								let _ = new_doc.pages_mut().copy_page_range_from_document(
									&temp_src_doc, 
									0..=0, 
									destination_index
								);
								has_content = true;
								//println!("图片通过 Rust 中转成功: {:?}", path.file_name().unwrap());
							}
						}
					}
				}
			}//endfor

			if has_content {
				println!("合并PDF总页数: {}", new_doc.pages().len());

				let save_res = new_doc.save_to_file(&save_path);

				match save_res {
					Ok(_) => println!("保存成功: {:?}", save_path),
					Err(e) => eprintln!("保存失败: {:?}", e),
				}
			}

			//if let Err(e) = new_doc.save_to_file(&save_path) {
			//	eprintln!("Save failed: {:?}", e);
			//}
		}
	}
}
//OCR LaTeX
impl PdfApp {
	fn render_ocr_result_window(&mut self, ctx: &egui::Context) {
		// 1. 先检查有没有数据，有的话克隆一份，断开与 self 的引用关联
		let mut text = match &self.current_latex {
			Some(t) => t.clone(),
			None => return, // 没数据直接返回
		};

		let is_ocr_loading = *self.is_ocr_loading.lock().unwrap();
		let is_prev_loading = *self.is_preview_loading.lock().unwrap();
		// 2. 这里的变量用于记录用户是否修改了文本
		let mut changed = false;
		let input_id = egui::Id::new("latex_input_field");

		// 3. 渲染窗口
		egui::Window::new("ocr_res")
			.title_bar(false)
			//.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
			.resizable(false)
			.movable(true)
			.pivot(egui::Align2::CENTER_CENTER) 
			.default_pos(ctx.content_rect().center())
			.frame(egui::Frame::window(&ctx.style()).inner_margin(12.0))
			.show(ctx, |ui| {
				ui.vertical(|ui| {
					ui.label(egui::RichText::new("LaTeX Code").weak().size(10.0));
					ui.add_space(6.0);

					//// 渲染可编辑的文本框
					egui::ScrollArea::vertical()
						.id_salt("math_area")
						.max_height(300.0)
						.auto_shrink([false,true])
						.show(ui, |ui| {
							// 1. 定义视觉着色器
							let mut layouter = |ui: &egui::Ui, string: &dyn egui::TextBuffer, wrap_width: f32| { 
								let mut job = egui::text::LayoutJob::default();

								// 因为 string 是 TextBuffer，我们需要调用 .as_str() 拿到字符串
								let text_content = string.as_str(); 

								// 获取颜色
								let default_color = ui.visuals().widgets.active.fg_stroke.color; 
								let dim_color = egui::Color32::from_gray(120);

								// 定义两组可能的边界
								let markers = [
									("\\begin{varwidth}{\\linewidth}", "\\end{varwidth}"),
									("\\begin{document}", "\\end{document}"),
								];

								let mut found_range = None;
								for (start_m, end_m) in markers {
									if let (Some(s), Some(e)) = (text_content.find(start_m), text_content.rfind(end_m)) {
										// 确保 end 在 start 之后
										if e > s {
											found_range = Some((s + start_m.len(), e));
											break; // 优先匹配 varwidth (数组第一个)
										}
									}
								}

								if let Some((body_start, body_end)) = found_range {
									// --- A. 渲染背景噪音 (头部分) ---
									job.append(
										&text_content[..body_start],
										0.0,
										egui::TextFormat {
											font_id: egui::FontId::monospace(12.0),
											color: dim_color,
											..Default::default()
										},
									);

									// --- B. 渲染核心正文 ---
									job.append(
										&text_content[body_start..body_end],
										0.0,
										egui::TextFormat {
											font_id: egui::FontId::monospace(14.0),
											color: default_color,
											..Default::default()
										},
									);

									// --- C. 渲染背景噪音 (尾部分) ---
									job.append(
										&text_content[body_end..],
										0.0,
										egui::TextFormat {
											font_id: egui::FontId::monospace(12.0),
											color: dim_color,
											..Default::default()
										},
									);
								} else {
									// --- D. 兜底：裸代码渲染 ---
									job.append(
										text_content,
										0.0,
										egui::TextFormat {
											font_id: egui::FontId::monospace(13.0),
											color: default_color,
											..Default::default()
										},
									);
								}


								job.wrap.max_width = wrap_width;
								ui.painter().layout_job(job)
							};

							// 2. 在 TextEdit 中调用
							let edit_res = ui.add(
								egui::TextEdit::multiline(&mut text)
								.id_salt(input_id)
								.hint_text("Type your math problem here")
								.desired_rows(6)  
								.desired_width(380.0) 
								.frame(false)
								.layouter(&mut layouter)
							);

							// 只有当 current_latex 为空（冷启动）时才自动聚焦
							if self.show_ocr_window && !edit_res.has_focus() {
								if text.is_empty() {
									edit_res.request_focus();
								}
							}

							// --- 手动边缘滚动逻辑 ---
							if edit_res.dragged() {
								if let Some(pointer_pos) = ui.ctx().pointer_interact_pos() {
									let scroll_rect = ui.clip_rect();

									// 1. 大幅度增加感应区（30-40像素），防止手指/鼠标太快划出去
									let margin = 35.0; 
									let mut scroll_delta = 0.0;

									// 2. 向上滚动判定：只要鼠标在顶部边缘附近（甚至稍微超出一点点也没关系）
									if pointer_pos.y < (scroll_rect.min.y + margin) {
										// 越往上拖，滚得越快
										let dist = (scroll_rect.min.y + margin - pointer_pos.y).max(2.0);
										scroll_delta = dist * 2.5; // 正值 = 向上滚动
									} 
									// 3. 向下滚动判定
									else if pointer_pos.y > (scroll_rect.max.y - margin) {
										let dist = (pointer_pos.y - (scroll_rect.max.y - margin)).max(2.0);
										scroll_delta = -dist * 2.5; // 负值 = 向下滚动
									}

									if scroll_delta != 0.0 {
										// 强制告诉 ui 滚动
										ui.scroll_with_delta(egui::vec2(0.0, scroll_delta));
									}
								}
							}

							if edit_res.changed() { changed = true; }

						});

					if is_ocr_loading {
						ui.add_space(4.0);
						ui.horizontal(|ui| {
							ui.add(egui::Spinner::new().size(12.0)); // 小一点，精致
							ui.label(
								egui::RichText::new(" AI is thinking...")
								.size(11.0)
								.color(egui::Color32::from_rgb(200, 200, 200)) // 淡淡的灰色，不抢眼
								.italics()
							);
						});
					}

					ui.add_space(10.0);
					ui.separator(); 
					ui.add_space(8.0);

					ui.columns(4, |columns| {
						// 第一列：放置 Close 按钮
						columns[0].vertical_centered(|ui| {
							if ui.button("Close").clicked() {
								self.show_ocr_window = false;
							}
						});


						// 第二列：放置 Copy 按钮
						columns[1].vertical_centered(|ui| {
							if ui.button("Copy").clicked() {
								if let Ok(mut cb) = arboard::Clipboard::new() {
									let _ = cb.set_text(text.clone());
								}
								self.show_ocr_window = false; 
							}
						});


						//// 第三列：预览LaTeX代码按钮
						columns[2].vertical_centered(|ui| {
							let is_empty = text.trim().is_empty();
							let preview_btn = ui.add_enabled(!is_empty && !is_prev_loading, egui::Button::new("Preview"));
							if preview_btn.clicked() && !is_prev_loading {
								let normalized = normalize_latex(&text);
								text = normalized.clone(); 
								self.current_latex = Some(normalized.clone());
								self.preview_texture = None;
								self.show_preview_window = true;
								self.solve_latex_preview(normalized, ctx.clone());
							}
							if is_empty {
								preview_btn.on_hover_text("Please enter LaTeX code to preview.");
							} else if is_prev_loading {
								preview_btn.on_hover_text("Preview is being generated...");
							}
						});

						// 第四列：More  (整合所有 AI 功能)
						columns[3].vertical_centered(|ui| {
							let is_empty = text.trim().is_empty();
							let has_preview = self.preview_texture.is_some();
							// 使用淡蓝色突出 More 菜单
							let more_text = egui::RichText::new("More").strong().color(egui::Color32::from_rgb(100, 180, 240));

							ui.menu_button(more_text, |ui| {
								ui.set_min_width(120.0);
								ui.spacing_mut().item_spacing.y = 5.0;

								ui.add_enabled_ui(!is_empty && !is_prev_loading, |ui| {
									// --- 1. 保存预览图片 ---
									// 只有当 Preview 生成了纹理才启用
									ui.add_enabled_ui(has_preview, |ui| {
										if ui.button("Save Preview as PNG").clicked() {
											//self.save_image_with_format("png"); 
											let img = self.preview_image.clone(); 
											self.save_generic_image(img, "Preview", "png");
											ui.close_kind(egui::UiKind::Menu);
										}

										ui.separator();

										if ui.button("Save Preview as JPG").clicked() {
											//self.save_image_with_format("jpg");
											let img = self.preview_image.clone(); 
											self.save_generic_image(img, "Preview", "jpg");
											ui.close_kind(egui::UiKind::Menu);
										}
									});

									//ui.separator();

									//// --- 2. Solve math problem  ---
									//let text_only = egui::RichText::new("Solve (Text Only)").strong()
									//	.color(egui::Color32::from_rgb(255, 165, 0));
									//let text_only_btn = ui.button(text_only);
									//if text_only_btn.clicked() {
									//	let source = SolveSource::Text(text.clone());
									//	self.solve_math_problem(source, ctx.clone());
									//	ui.close_kind(egui::UiKind::Menu);
									//}

									//ui.separator();

									//// --- 3. 图文并茂解题 ---
									//let both_text = egui::RichText::new("Solve (Image + Text)").strong()
									//	.color(egui::Color32::from_rgb(255, 165, 0));
									//let both_btn = ui.button(both_text);
									//if both_btn.clicked() {
									//	let image_to_send = self.last_ocr_image.as_ref()
									//		.or(self.last_cropped_image.as_ref())
									//		.cloned();

									//	if let Some(img) = image_to_send {
									//		// 此时框内文字可能是 OCR 结果，也可能是用户改过的指令
									//		let dynamic_img = image::DynamicImage::ImageRgba8(img.clone());
									//		let source = SolveSource::Both(text.clone(), dynamic_img);
									//		self.solve_math_problem(source, ctx.clone());
									//	}
									//	ui.close_kind(egui::UiKind::Menu);
									//}
									//both_btn.on_hover_text("Use the crop image for context and the text for instructions.");
								});

								//ui.separator();

								// --- 4. 翻译选项 ---
								//ui.add_enabled_ui(!is_empty && !is_ocr_loading, |ui| {
								//	if ui.button("Translate to English").clicked() {
								//		self.run_text_translation(&text, "English", ctx.clone());
								//		ui.close_kind(egui::UiKind::Menu);
								//	}
								//	ui.separator();
								//	if ui.button("Translate to Chinese").clicked() {
								//		self.run_text_translation(&text, "Chinese", ctx.clone());
								//		ui.close_kind(egui::UiKind::Menu);
								//	}
								//	ui.separator();
								//	if ui.button("Translate to Spanish").clicked() {
								//		self.run_text_translation(&text, "Spanish", ctx.clone());
								//		ui.close_kind(egui::UiKind::Menu);
								//	}
								//});

							});
						});

					});
				});
			});

		// 4. 【关键】如果闭包里的文本变了，在闭包外面更新 self
		if changed {
			self.current_latex = Some(text);
		}
	}
}
//AI OCR
impl PdfApp {
fn run_latex_ocr(&self, img: image::RgbaImage, ctx: egui::Context) {
    // 1. 根据 Provider 选择对应的函数和 Prompt
    if self.ocr_provider == "local" {
        // 本地版本：使用专用的本地执行函数
				let prompt = "OCR:";
        self.execute_vision_task_local(img, ctx, prompt);
    } else {
        // 云端版本：使用 OpenAI 风格的通用函数
        let prompt = "Please recognize the text and formulas in this image and output them in standard LaTeX format. Do not solve it";
        self.execute_vision_task_cloud(img, ctx, prompt);
    }
}
}
//在线模型
impl PdfApp {
fn execute_vision_task_cloud(&self, img: image::RgbaImage, ctx: egui::Context, prompt: &'static str) {
    if *self.is_ocr_loading.lock().unwrap() {
        return;
    }
    
    // 提取配置
    let api_url = self.ocr_api_url.clone();
    let api_token = self.ocr_api_token.clone();
    let api_model = self.ocr_model_name.clone();

    let ocr_res = Arc::clone(&self.ocr_result);
    let loading = Arc::clone(&self.is_ocr_loading);
    let ctx_clone = ctx.clone();

    // 双重检查锁定并设置状态
    {
        let mut loading_lock = loading.lock().unwrap();
        if *loading_lock { return; }
        *loading_lock = true;
    }

    std::thread::spawn(move || {
        // 1. 缩放图片逻辑保持不变
				let img_to_send = scale_image_for_ocr(img);

        // 2. 编码图片为 Base64
        let mut buffer = std::io::Cursor::new(Vec::new());
        if let Err(e) = img_to_send.write_to(&mut buffer, image::ImageFormat::Png) {
            eprintln!("Image encoding failed: {}", e);
            *loading.lock().unwrap() = false;
            return;
        }
        let image_bytes = buffer.into_inner();
        let base64_image = base64::engine::general_purpose::STANDARD.encode(image_bytes);

        // 3. 构造 OpenAI 风格的 Payload (移除 local 分支)
        let image_data_url = format!("data:image/png;base64,{}", base64_image);
        let payload = serde_json::json!({
            "model": &api_model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": prompt },
                    { "type": "image_url", "image_url": { "url": image_data_url } }
                ]
            }]
        });

        // 4. 发送请求 (默认添加 Authorization)
        let client = reqwest::blocking::Client::new();
        let response = client.post(&api_url)
            .header("Authorization", format!("Bearer {}", api_token.trim()))
            .json(&payload)
            .send();

        // 5. 处理结果
        match response {
            Ok(res) => {
                let status = res.status();
                let v: serde_json::Value = res.json().unwrap_or_default();

                if status.is_success() {
                    // 仅保留 OpenAI 风格的路径：v["choices"][0]["message"]["content"]
                    if let Some(content) = v["choices"][0]["message"]["content"].as_str() {
                        
                        // 代码块提取逻辑保持不变
                        let extracted = if let Some(start) = content.find("```") {
                            let content_after = &content[start + 3..];
                            let body = content_after.split_once('\n')
                                .map(|x| x.1)
                                .unwrap_or(content_after);

                            body.split("```").next().unwrap_or(body).to_string()
                        } else {
                            content.to_string()
                        };

                        let final_result = extracted.trim().to_string();

                        {
                            let mut res_lock = ocr_res.lock().unwrap();
                            *res_lock = Some(final_result);
                            *loading.lock().unwrap() = false;
                        }
                        ctx_clone.request_repaint();
                    } else {
                        eprintln!("Failed to parse content from response: {:?}", v);
                        *loading.lock().unwrap() = false;
                    }
                } else {
                    let error_msg = v["error"]["message"]
                        .as_str()
                        .or(v["error"].as_str())
                        .unwrap_or("Unknown API Error");
                    eprintln!("API Error ({}): {}", status, error_msg);
                    *loading.lock().unwrap() = false;
                }
            }
            Err(e) => {
                eprintln!("❌ Network request failed: {}", e);
                *loading.lock().unwrap() = false;
            }
        }
    });
}
}
//本地模型
impl PdfApp {
	fn execute_vision_task_local(&self, img: image::RgbaImage, ctx: egui::Context, prompt: &'static str) {
		// 1. 状态检查
		if *self.is_ocr_loading.lock().unwrap() {
			return;
		}

		let model_id = self.ocr_model_name.clone();
		let ocr_res = Arc::clone(&self.ocr_result);
		let loading = Arc::clone(&self.is_ocr_loading);
		let ctx_clone = ctx.clone();

		// 设置加载状态
		{
			let mut loading_lock = loading.lock().unwrap();
			if *loading_lock { return; }
			*loading_lock = true;
		}

		std::thread::spawn(move || {
			let result = (|| -> Option<String> {
				// --- A. 图像预处理与临时保存 ---
				let img_to_send = scale_image_for_ocr(img);

				// 保存到临时文件供外部 EXE 读取
				let temp_path = std::env::temp_dir().join("ocr_local_input.png");
				img_to_send.save(&temp_path).ok()?;
				let img_path_str = temp_path.to_string_lossy().to_string();

				// --- B. 获取本地路径与执行 ---
				let (exe, model, mmproj) = get_ocr_paths(&model_id)?;

				let output = std::process::Command::new(exe)
					.arg("-m").arg(model)
					.arg("--mmproj").arg(mmproj)
					.arg("--image").arg(&img_path_str)
					.arg("--temp").arg("0.1")
					.arg("-p").arg(prompt)
					.arg("-n").arg("1024")
					.output().ok()?;

				if !output.status.success() { return None; }

				// --- C. 清洗输出 ---
				let raw = String::from_utf8_lossy(&output.stdout);
				let cleaned = raw.lines()
					.filter(|line| !line.is_empty() && !line.starts_with("llama_"))
					.collect::<Vec<_>>()
					.join("\n");
				Some(cleaned.trim().to_string())
			})();

			// --- D. 结果回传 ---
			{
				let mut res_lock = ocr_res.lock().unwrap();
				let mut loading_lock = loading.lock().unwrap();

				*res_lock = result.or_else(|| Some("OCR 运行失败".to_string()));
				*loading_lock = false;
			}
			ctx_clone.request_repaint();
		});
	}
}
fn get_ocr_paths(model_id: &str) -> Option<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf)> {
	let home = dirs::home_dir()?;
	let data_local = dirs::data_local_dir()?;

	let exe_path = {
		let local_bin = home.join(".local/bin/llama-mtmd-cli");
		if local_bin.exists() {
			local_bin
		} else {
			std::env::current_exe().ok()?
				.parent()?
				.join("llama-mtmd-cli")
		}
	};

	let model_base = data_local.join("llama_models");
	//let model_path = model_base.join("GLM-OCR-Q8_0.gguf");
	//let mmproj_path = model_base.join("mmproj-GLM-OCR-Q8_0.gguf");
	let model_path = model_base.join(format!("{}.gguf", model_id));
	let mmproj_path = model_base.join(format!("mmproj-{}.gguf", model_id));

	// 只要有一个文件不存在，就返回 None
	if !exe_path.exists() || !model_path.exists() {
		return None;
	}

	Some((exe_path, model_path, mmproj_path))
}
fn scale_image_for_ocr(img: image::RgbaImage) -> image::RgbaImage {
	let (w, h) = (img.width() as f32, img.height() as f32);
	let max_dim = 1340.0;
	let min_w = 890.0;

	if w <= max_dim && h <= max_dim {
		return img;
	}

	let mut scale = (max_dim / w).min(max_dim / h);
	if w * scale < min_w && w > min_w {
		scale = min_w / w;
	}

	if scale >= 1.0 { return img; }

	image::DynamicImage::ImageRgba8(img)
		.resize((w * scale) as u32, (h * scale) as u32, image::imageops::FilterType::CatmullRom)
		.to_rgba8()
}

//LaTeX代码预览
impl PdfApp {
    fn render_page_to_texture(
        &self, 
        ctx: &egui::Context, 
        page: &pdfium_render::prelude::PdfPage, 
        target_width: u32
		) -> Option<(egui::TextureHandle, image::RgbaImage)> {
        let render_config = PdfRenderConfig::new()
            .set_target_width(target_width as i32)
						.set_maximum_height(2000)
            .set_clear_color(PdfColor::WHITE); // 预览一定要白底

        let bitmap = page.render_with_config(&render_config).ok()?;
        let image = bitmap.as_image().to_rgba8();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        );

				let texture = ctx.load_texture(
					"pdf_render_temp",
					color_image,
					egui::TextureOptions::LINEAR,
				);

				Some((texture, image))
    }
}
impl PdfApp {
fn render_latex_preview_window(&mut self, ctx: &egui::Context) {
    //if !self.show_preview_window { return; }

    let is_loading = *self.is_preview_loading.lock().unwrap();
    
		if is_loading {
			if self.preview_texture.is_some() {
				self.preview_texture = None; // 这会触发旧 TextureHandle 的 Drop，从而释放 GPU 显存
			}
		} 
		// 只有当编译结束 (is_loading == false) 且 纹理还没生成时，才去读 PDF
		else if self.preview_texture.is_none() {
			let pdf_path = get_app_data_dir().join("preview.pdf");
			if pdf_path.exists() {
				if let Ok(doc) = self.pdfium.load_pdf_from_file(&pdf_path, None) {
					if let Ok(page) = doc.pages().get(0) {
						if let Some((texture, raw_img)) = self.render_page_to_texture(ctx, &page, 800) {
							self.preview_texture = Some(texture);
							self.preview_image = Some(raw_img);
						}
					}
				}
			}
		}

		// 2. 渲染紧凑型窗口
    egui::Window::new("latex_preview_popup")
        .title_bar(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]) // 居中锚定
        .pivot(egui::Align2::CENTER_CENTER)           // 确保缩放时以中心点为准
        .resizable(false)
        .auto_sized() 
        .default_width(100.0) // --- 关键：初始给一个很小的宽度，让它向上“撑开”而不是向下“收缩” ---
        .max_width(450.0)    // 限制最大宽度
        //.frame(egui::Frame::window(&ctx.style()).inner_margin(16.0))
        .show(ctx, |ui| {
            // 强制内部内容水平居中，防止抖动
            ui.vertical_centered(|ui| {
							egui::ScrollArea::vertical()
								.max_height(400.0) // 这里设置你想要的最大高度
								.auto_shrink([true, true]) // 如果内容没那么高，窗口会自动收缩
								.show(ui, |ui| {
									ui.vertical_centered(|ui| { // 确保滚动区内部也居中
                // --- 公式显示区 ---
										if is_loading {
											// 状态 A: 加载中
											ui.add(egui::Spinner::new().size(20.0));
											ui.label("Compiling...");
										} else if let Some(err) = &*self.latex_error.lock().unwrap() {
											// 状态 B: 编译报错
											ui.spacing_mut().item_spacing.y = 8.0;
											ui.label(egui::RichText::new("⚠ LaTeX Error").color(egui::Color32::RED).strong());
											ui.add_space(4.0);
											ui.allocate_ui(egui::vec2(280.0, 0.0), |ui| {
												ui.label(egui::RichText::new(err).color(egui::Color32::LIGHT_RED));
											});
										} else if let Some(texture) = &self.preview_texture {
											// 状态 C: 正常显示图片
											egui::Frame::canvas(ui.style())
												.fill(egui::Color32::WHITE)
												.inner_margin(8.0)
												.show(ui, |ui| {
													let display_size = texture.size_vec2() * 0.5;
													ui.add(egui::Image::from_texture(texture).fit_to_exact_size(display_size));
												});
										}
									});
								});

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // --- 底部按钮区 ---
                ui.horizontal(|ui| {
                    // 计算按钮区的最小宽度，防止窗口缩得比按钮还窄
                    let btn_width = 120.0; 
                    ui.set_min_width(btn_width);
                    
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                         if ui.button("📋 Copy").clicked() {
													 if let Some(img) = &self.preview_image {
														 copy_image_to_clipboard(img); 
													 }
													 self.show_preview_window = false;
                        }
                        
                        ui.add_space(8.0); // 按钮间固定间距

												ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
													if ui.button("Close").clicked() {
														self.show_preview_window = false;
													}
												});
                    });
                });
            });
        });
}
fn solve_latex_preview(&self, latex_query: String, ctx: egui::Context) {
    let loading = Arc::clone(&self.is_preview_loading);
		let error_arc = Arc::clone(&self.latex_error);
    let ctx_clone = ctx.clone();

    // 编译前先清理旧文件
		let app_dir = get_app_data_dir(); 
    let pdf_path = app_dir.join("preview.pdf");
    let _ = std::fs::remove_file(pdf_path);

    {
			*loading.lock().unwrap() = true;
			*error_arc.lock().unwrap() = None;
		}

		std::thread::spawn(move || {
        match compile_to_pdf(&latex_query) {
            Ok(_) => {
                *loading.lock().unwrap() = false;
            }
            Err(e) => {
                *error_arc.lock().unwrap() = Some(e);
                *loading.lock().unwrap() = false;
            }
        }
        ctx_clone.request_repaint();
    });
}
}
fn normalize_latex(content: &str) -> String {
	let trimmed = content.trim();
	if trimmed.is_empty() { return String::new(); }

	let has_header = trimmed.contains("\\documentclass");
	let has_begin = trimmed.contains("\\begin{document}");

	if has_header && has_begin {
		// 已经是完整格式，不做变动（或者只补全 \end{document}）
		if trimmed.contains("\\end{document}") {
			trimmed.to_string()
		} else {
			format!("{}\n\\end{{document}}", trimmed)
		}
	} else {
		// 片段格式：包装成标准 standalone 格式并返回
		// 这样用户在 UI 里就能看到完整的代码结构，方便他们添加其他宏包
		format!(
			r#"\documentclass[border=2pt, preview]{{standalone}}
\usepackage{{amsmath, amsfonts, amssymb, bm,enumitem, tikz, varwidth}}
\setenumerate[1]{{itemsep=5pt,partopsep=0pt,parsep=5pt,topsep=5pt}}
\setitemize[1]{{itemsep=5pt,partopsep=0pt,parsep=5pt,topsep=5pt, leftmargin=2pc}}
\begin{{document}}
\begin{{varwidth}}{{\linewidth}}

{}

\end{{varwidth}}
\end{{document}}"#,
				trimmed
		)
	}
}
fn compile_to_pdf(content: &str) -> Result<Vec<u8>, String> {
	let temp_dir = get_app_data_dir(); // 获取 ~/.local/share/vectorsnap
	let tex_path = temp_dir.join("preview.tex");
	fs::write(&tex_path, content).map_err(|e| format!("Failed to write .tex: {}", e))?;

	// 配置通用参数
	let mut cmd = std::process::Command::new("pdflatex");
	cmd.current_dir(&temp_dir)
		.arg("-interaction=nonstopmode")
		.arg("-halt-on-error")
		.arg("preview.tex");

	#[cfg(target_os = "windows")]
	{
		use std::os::windows::process::CommandExt;
		// CREATE_NO_WINDOW = 0x08000000 防止弹出控制台黑框
		cmd.creation_flags(0x08000000);
	}

	let output = cmd.output().map_err(|e| format!("pdflatex error: {}", e))?;

	if output.status.success() {
		fs::read(temp_dir.join("preview.pdf")).map_err(|e| format!("Read PDF error: {}", e))
	} else {
		// ... 错误捕获逻辑 ...
		let out_log = String::from_utf8_lossy(&output.stdout);
		let err_log = String::from_utf8_lossy(&output.stderr);
		let full_log = format!("{}\n{}", out_log, err_log);

		let lines: Vec<&str> = full_log.lines().collect();
		let mut error_report = Vec::new();

		for i in 0..lines.len() {
			if lines[i].starts_with('!') {
				// 捕获错误主体
				error_report.push(lines[i]);
				// 尝试捕获接下来的 2 行上下文（比如行号、具体出错的符号）
				if i + 1 < lines.len() { error_report.push(lines[i+1]); }
				if i + 2 < lines.len() { error_report.push(lines[i+2]); }
				break; // 通常第一个错误是最核心的，抓到一个完整的就够了
			}
		}

		let error_msg = error_report.join("\n");

		Err(if error_msg.is_empty() { "LaTeX Syntax Error (Check Log)".into() } else { error_msg })
	}
}
fn get_app_data_dir() -> std::path::PathBuf {
    // 使用 dirs 库自动适配所有操作系统：
    // Linux:   /home/user/.local/share/vectorsnap
    // macOS:   /Users/user/Library/Application Support/vectorsnap
    // Windows: C:\Users\user\AppData\Roaming\vectorsnap\data
    let app_dir = dirs::data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap())
        .join("vectorsnap");

    let _ = std::fs::create_dir_all(&app_dir);
    app_dir
}

//Synctex反向搜索
impl PdfApp {
	pub fn sync_to_latex(&self, click_pos: egui::Pos2, ui_rect: egui::Rect) {
		let path = match &self.pdf_path {
			Some(p) => p,
			None => return,
		};

		let page_index = self.current_page;
		let Some(doc) = &self.pdf_doc else { return };
		let Ok(page) = doc.pages().get(page_index as u16) else { return };

		// 1. 计算相对比例 (0.0 - 1.0)
		let local_x = (click_pos.x - ui_rect.min.x) / ui_rect.width();
		let local_y = (click_pos.y - ui_rect.min.y) / ui_rect.height();

		// 2. 获取 PDF 页面原始尺寸 (Points, 72 DPI)
		// 考虑旋转逻辑，与你 perform_high_res_crop 保持一致
		let angle = self.rotations.get(page_index).copied().unwrap_or(0.0);
		let is_sideways = ((angle / 90.0).round() as i32).abs() % 2 != 0;

		let (p_w, p_h) = if is_sideways {
			(page.height().value, page.width().value)
		} else {
			(page.width().value, page.height().value)
		};

		// 3. 映射到 PDF 坐标系 (注意：SyncTex 的 y 轴通常是从顶端向下的点数)
		let target_x = local_x * p_w;
		let target_y = local_y * p_h;

		// 4. 调用 synctex 命令行工具
		// 这里的命令：synctex edit -o "页码:x:y:pdf文件名"
		let output = Command::new("synctex")
			.arg("edit")
			.arg("-o")
			.arg(format!("{}:{}:{}:{}", page_index + 1, target_x, target_y, path.display()))
			.output();

		if let Ok(out) = output {
			let result = String::from_utf8_lossy(&out.stdout);
			//println!("SyncTex Result: {}", result);

			// 5. 解析结果并调用编辑器 (以 VS Code 为例)
			// SyncTex 返回格式通常包含: 'Input:main.tex', 'Line:123', 'Column:1'
			self.open_editor_from_synctex(&result);
		}
	}

	fn open_editor_from_synctex(&self, synctex_output: &str) {
		let mut file = "";
		let mut line = "";

		// 解析 SyncTex 输出
		for l in synctex_output.lines() {
			if l.starts_with("Input:") { file = l[6..].trim(); }
			if l.starts_with("Line:") { line = l[5..].trim(); }
		}

		if !file.is_empty() && !line.is_empty() {
			// 从配置中读取命令模板，例如: "gvim --remote-silent +{line} {file}"
			// 默认值可以设为 gvim
			let command_template = self.config.editor_command.clone(); 

			// 替换占位符
			let final_command = command_template
				.replace("{file}", file)
				.replace("{line}", line);

			// 执行 shell 命令
			//// 在 Linux 下使用 sh -c 可以方便地处理复杂的带参数命令
			#[cfg(target_os = "linux")]
			let _ = std::process::Command::new("sh")
				.arg("-c")
				.arg(final_command)
				.spawn();

			#[cfg(target_os = "windows")]
			let _ = std::process::Command::new("cmd")
				.arg("/C")
				.arg(final_command)
				.spawn();
		}
	}
}

//主循环
impl eframe::App for PdfApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    if let Ok(mut reload) = self.needs_reload.lock() {
        if *reload {
						self.reload_retries = 10;
            *reload = false; // 消费重载请求，重置为 false
        }
    }

		if self.reload_retries > 0 {
			if let Some(path) = self.pdf_path.clone() {
				if self.load_pdf_path(path) {
					// 成功：清理状态
					self.texture = None;
					self.reload_retries = 0;
					ctx.request_repaint();
				} else {
					self.reload_retries -= 1;
					if self.reload_retries > 0 {
						ctx.request_repaint_after(std::time::Duration::from_millis(1));
					} 
				}
			}
		}

		// 发送命令给窗口管理器更新标题
		let current_title = self.pdf_name.as_deref().unwrap_or("VectorSnap").to_string();
		if self.last_applied_title != current_title {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(current_title.clone()));
        self.last_applied_title = current_title;
    }

		// --- 0. 快捷键监听 (放在所有 UI 渲染之前) ---
		self.handle_shortcuts(ctx);
		// --- A. 处理文件拖拽事件 ---
		ctx.input(|i| {
			// 如果有文件被松开拖入窗口
			if !i.raw.dropped_files.is_empty() {
				if let Some(file) = i.raw.dropped_files.get(0) {
					if let Some(path) = &file.path {
						// 简单的后缀检查
						let is_pdf = path.extension()
							.and_then(|s| s.to_str())
							.map(|s| s.to_lowercase()) // 转为小写
							== Some("pdf".to_string());

						if is_pdf {
							self.load_pdf_path(path.clone());
						}
					}
				}
			}
		});

		// --- B. 增强视觉反馈（可选）：当文件悬停在窗口上方时显示提示 ---
		if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
			egui::Area::new(egui::Id::new("drop_target"))
				.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
				.order(egui::Order::Foreground) // 确保在最上层
				.show(ctx, |ui| {
					ui.label(
						egui::RichText::new("Drop PDF here to open")
						.size(32.0)
						.color(egui::Color32::LIGHT_BLUE)
						.background_color(egui::Color32::from_black_alpha(150))
					);
				});
		}
		// 1. 顶部面板
		self.render_top_panel(ctx);

		// 2. 中央 PDF 显示区域
		egui::CentralPanel::default().show(ctx, |ui| {
			if self.pdf_doc.is_some() {
				self.render_pdf_content(ui, ctx);
			} else {
				ui.vertical_centered(|ui| {
					ui.add_space(50.0);

					ui.heading("Welcome to PDF Reader VectorSnap");
					ui.add_space(20.0);

					// Turn the text into a clickable button
					let open_btn = egui::Button::new(
						egui::RichText::new("📂 Open PDF")
						.size(24.0)
						.color(egui::Color32::LIGHT_BLUE)
					);

					if ui.add(open_btn).clicked() {
						// Trigger the file picker dialog
						if let Some(path) = rfd::FileDialog::new()
							.add_filter("PDF files", &["pdf", "PDF"])
								.pick_file() 
						{
							self.load_pdf_path(path);
						}
					}

					ui.add_space(20.0);
					ui.label("or Drop a PDF file here to start reading.");

					ui.add_space(50.0);
					self.render_favorites(ui);
				});
			}
		});

		// 3. 裁剪预览弹窗
		self.render_preview_window(ctx);

		// 4. 调用帮助窗口
		if self.show_help_window {
			self.render_help_window(ctx);
		}

		if self.show_export_window {
			self.render_export_window(ctx);
		}
		if self.show_merge_window {
			self.render_merge_window(ctx);
		}

		if let Ok(mut lock) = self.ocr_result.try_lock() {
        if let Some(new_content) = lock.take() {
            self.current_latex = Some(new_content); // 存到本地
            self.show_ocr_window = true;           // 弹窗打开
        }
    }

		if self.show_ocr_window {
			self.render_ocr_result_window(ctx);
		}

    if self.show_preview_window { 
			self.render_latex_preview_window(ctx);
		}
	}
	fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
		self.record_current_position();
	}
}
//main函数
fn main() -> eframe::Result<()> {
	// 1. 提取命令行参数
	let args: Vec<String> = std::env::args().collect();

	// 2. 尝试获取路径（注意：索引 0 是程序名，索引 1 才是文件路径）
	let initial_pdf_path = if args.len() > 1 {
		Some(std::path::PathBuf::from(&args[1]))
	} else {
		None
	};

	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_active(true)
			.with_inner_size([900.0, 1000.0]),
			run_and_return: true,
			..Default::default()
	};

	eframe::run_native(
		"VectorSnap",
		options,
		Box::new(move |cc| {
			#[cfg(target_os = "windows")]
			{
				let monitor_size = cc.egui_ctx.input(|i| i.viewport().monitor_size)
					.unwrap_or(egui::vec2(1280.0, 720.0));

				let target_height = 1020.0f32.min(monitor_size.y  - 80.0);

				cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
						egui::vec2(800.0, target_height)
				));

				cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
						egui::pos2(100.0, 0.0)
				));
			}
			setup_fonts(&cc.egui_ctx);
			Ok(Box::new(PdfApp::new(cc, initial_pdf_path)))
		}),
		)
}
