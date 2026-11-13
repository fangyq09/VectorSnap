#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
//use pdfium_render::prelude::*;
use pdfium_render::prelude::{Pdfium, PdfDocument, PdfPageRenderRotation, PdfRenderConfig, 
PdfPageObjectsCommon, PdfPageAnnotationCommon, PdfPageObjectType};
use std::path::PathBuf;
use std::collections::{HashMap};
use std::sync::{mpsc, Arc, Mutex};
use notify::{Watcher, RecursiveMode};
//use interprocess::local_socket::{prelude::*, ListenerOptions, Stream};
//use std::io::{Read, Write};
use std::thread;
use std::process::{Child, Command};
//use std::hash::{Hash, Hasher};
//use std::collections::hash_map::DefaultHasher;
//use base64::Engine;
//use std::fs;
//use image::{GenericImageView};

mod crop;
mod synctex;
mod latex_preview;
mod merge_pdf;
mod handwriting;
mod export;
mod ai_tools;
mod ipc;

use crate::handwriting::{
    //EraseStroke,
    //Stroke,
    ToolMode,
    StrokePoint,
    ColorShape,
    Annotation,
};
use crate::crop::copy_image_to_clipboard;
use crate::ipc::{IpcMessage, IpcStartup, IpcHandle, setup_ipc, pdf_ipc_hash, parse_args};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// 根据操作系统嵌入不同的库文件
#[cfg(target_os = "linux")]
const PDFIUM_LIB_BYTES: &[u8] = include_bytes!("../assets/libpdfium.so");

#[cfg(target_os = "windows")]
const PDFIUM_LIB_BYTES: &[u8] = include_bytes!("../assets/pdfium.dll");

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AppConfig {
	#[serde(default = "default_true")]
	pub show_favorites: bool,

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

	#[serde(default = "default_true")]
	pub auto_crop_dpi: bool,

	#[serde(default)]
	pub synctex_dir: String,

	#[serde(default = "default_nvim_socket")]
	pub nvim_socket: String,

	#[serde(default = "default_nvim_path")]
	pub nvim_path: String,

	#[serde(default)]
	pub vim_function: Option<String>,

}

fn default_true() -> bool {
    true
}

fn default_nvim_socket() -> String {
    "/tmp/nvim_tex.sock".to_string()
}

fn default_nvim_path() -> String {
    "nvim".to_string()
}

#[cfg(target_os = "windows")]
fn default_editor_command() -> String {
    "gvim.exe".to_string()
}

#[cfg(target_os = "linux")]
fn default_editor_command() -> String {
    "gvim".to_string()
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
			show_favorites: true,
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
			auto_crop_dpi: true,
			synctex_dir: String::new(),
			nvim_socket: default_nvim_socket(),
			nvim_path: "nvim".to_string(),
			vim_function: None,
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

fn spawn_and_reap(child: std::io::Result<Child>) {
    if let Ok(mut child) = child {
        let _ = thread::spawn(move || {
            let _ = child.wait();
        });
    }
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
		last_selection_rect: Option<egui::Rect>,
		//favorite_files: Vec<PathBuf>,
		//favorite_folders: Vec<PathBuf>,
		//show_full_path: bool,
		//editor_command: String,
		rotations: Vec<f32>,
		show_help_window: bool,
		show_settings_window: bool,
		last_rendered_page: usize,
    last_rendered_angle: f32,
		show_export_window: bool,
		export_range_text: String,
		show_merge_window: bool,
		merge_file_list: Vec<std::path::PathBuf>,
    force_a4: bool,
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
		//extracted_text_cache: Option<String>,
		enable_auto_crop_dpi: bool,
		ipc_receiver: Option<mpsc::Receiver<IpcMessage>>,
		ipc_handle: Option<IpcHandle>,
		vim_servername: Option<String>,
		editor: Option<String>,
		synctex: Option<String>,
		new_ocr_name: String,
    new_ocr_model_id: String,
    new_ocr_provider: String,
    new_ocr_api_url: String,
    new_ocr_api_token: String,
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
pub fn get_config_dir() -> std::path::PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap())
        .join("vectorsnap");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
fn config_path() -> std::path::PathBuf {
    get_config_dir().join("config.json")
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
	pub fn new(
		_cc: &eframe::CreationContext<'_>, 
		path: Option<std::path::PathBuf>,
		ipc_receiver: Option<mpsc::Receiver<IpcMessage>>,
		ipc_handle: Option<IpcHandle>,
		) -> Self {
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
		let initial_auto_crop_dpi = config.auto_crop_dpi;

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
			target_dpi: 300.0,
			view_scale: 1.0,
			zoom_factor: 1.0,
			last_opened_dir: None,
			drag_start_local: None,
			selection_rect_local: None,
			last_selection_rect: None,
			//favorite_files: config.favorite_files.clone(),
			//favorite_folders: config.favorite_folders.clone(),
			//show_full_path: config.show_full_path.clone(),
			//editor_command: config.editor_command.clone(),
			config,
			rotations: Vec::new(),
			show_help_window: false,
			show_settings_window: false,
			last_rendered_page: 999999, 
			last_rendered_angle: 0.0,
			show_export_window: false,
			export_range_text: "".to_string(),
			show_merge_window: false,
			merge_file_list: Vec::new(),
			force_a4: false,
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
			//extracted_text_cache: None,
			enable_auto_crop_dpi: initial_auto_crop_dpi,
			ipc_receiver,
			ipc_handle,
			vim_servername: None,
			editor: None,
			synctex: None,
			new_ocr_name: String::new(),
			new_ocr_model_id: String::new(),
			new_ocr_provider: "local".to_string(),
			new_ocr_api_url: String::new(),
			new_ocr_api_token: String::new(),
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
						//let mut page_str = (self.current_page + 1).to_string();
						//let res = ui.add(
						//	egui::TextEdit::singleline(&mut page_str)
						//		.id(ui.id().with("page_text"))
						//	.desired_width(35.0) // 限制宽度，防止输入框过长
						//);
						//if res.changed() {
						//	// 尝试解析用户输入的数字，并限制在 1..=total 范围内
						//	if let Ok(parsed_page) = page_str.parse::<usize>() {
						//		let bounded_page = parsed_page.clamp(1, total);
						//		self.current_page = bounded_page.saturating_sub(1);
						//		self.texture = None; // 触发重新渲染
						//	}
						//}
						let mut page_str = ui.data_mut(|d| {
							d.get_temp::<String>(ui.id().with("page_input"))
								.unwrap_or_else(|| (self.current_page + 1).to_string())
						});
						if !ui.memory(|mem| mem.has_focus(ui.id().with("page_text"))) {
							page_str = (self.current_page + 1).to_string();
						}
						ui.label("Page");
						let res = ui.add(
							egui::TextEdit::singleline(&mut page_str)
							.id(ui.id().with("page_text"))
							.desired_width(30.0) 
						);
						ui.label(format!("of {}", total));
						if res.changed() {
							page_str.retain(|c| c.is_ascii_digit());
							ui.data_mut(|d| d.insert_temp(ui.id().with("page_input"), page_str.clone()));
						}
						if res.lost_focus() {
							let parsed_page = page_str.parse::<usize>().unwrap_or(self.current_page + 1);
							let bounded_page = parsed_page.clamp(1, total);
							self.current_page = bounded_page.saturating_sub(1);
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
								let mut auto_checked = self.enable_auto_crop_dpi;
								if ui.checkbox(&mut auto_checked, "🔄 Auto DPI").changed() {
									self.enable_auto_crop_dpi = auto_checked;
									self.config.auto_crop_dpi = auto_checked; // 同步到 config
									self.save_all_config();
								}
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
								ui.menu_button("Settings", |ui| {
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

							ui.separator();
							if ui.button("⚙ Settings").clicked() {
								self.show_settings_window = true;
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
			let stroke_width = if is_selected { 2.0_f32 } else { 1.0_f32 };

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
//设置窗口
impl PdfApp {
	fn render_settings_window(&mut self, ctx: &egui::Context) {
		let mut is_open = self.show_settings_window;
		let mut should_close = false;
		let mut should_cancel = false;

		egui::Window::new("⚙ App Settings")
			.open(&mut is_open)
			.resizable(true)          // 允许手动调整窗口大小
			.collapsible(false)
			.default_width(450.0)
			.default_height(600.0)    // 👈 设定默认高度
			.max_height(700.0)        // 👈 限制窗口最大高度，防止撑满整个屏幕
			.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
			.show(ctx, |ui| {
				// 👈 用 ScrollArea 包裹所有内容，超过高度时自动出现滚动条
				egui::ScrollArea::vertical()
					.auto_shrink([false, true])
					.show(ui, |ui| {
						ui.set_width(ui.available_width());
						ui.add_space(5.0);

						ui.heading("Preferences & Configuration");
						ui.add_space(10.0);
						ui.separator();
						ui.add_space(10.0);


						// 收藏夹
						ui.horizontal(|ui| {
							ui.label("Show Favorites Files and Directory:");
							let mut enable_show_favorites = self.config.show_favorites;
							if ui.checkbox(&mut enable_show_favorites, "Enable Show Favorites").changed() {
								self.config.show_favorites = enable_show_favorites;
							}
						});
						ui.add_space(15.0);

						// 1. SyncTeX 目录配置
						ui.horizontal(|ui| {
							ui.label("SyncTeX Directory:");
							ui.text_edit_singleline(&mut self.config.synctex_dir);
						});
						ui.weak("Leave empty if .synctex.gz is in the same folder as PDF.");
						ui.add_space(15.0);

						// SyncTeX 之后执行vim函数
						ui.horizontal(|ui| {
							ui.label("Vim Function:");
							let mut vim_function = self.config.vim_function.clone().unwrap_or_default();
							if ui.text_edit_singleline(&mut vim_function).changed() {
								self.config.vim_function = if vim_function.trim().is_empty() {
									None
								} else {
									Some(vim_function)
								};
							}
						});
						ui.weak("Calling a vim function after SyncTeX.");
						ui.add_space(15.0);

						// 2. Neovim Socket 路径配置 ---
						ui.horizontal(|ui| {
							ui.label("Neovim Socket Path:");
							ui.text_edit_singleline(&mut self.config.nvim_socket);
						});
						ui.weak("Path for Neovim IPC server (e.g., /tmp/nvim_tex.sock).");
						ui.add_space(15.0);

						// Neovim 可执行文件
						ui.horizontal(|ui| {
							ui.label("Neovim Executable:");
							ui.text_edit_singleline(&mut self.config.nvim_path);
						});
						ui.weak("Path to nvim executable used for RPC connections.");
						ui.add_space(15.0);

						// 3. 编辑器命令模板配置
						ui.horizontal(|ui| {
							ui.label("Editor Command Template:");
							//ui.text_edit_singleline(&mut self.config.editor_command);
							ui.add(
								egui::TextEdit::singleline(&mut self.config.editor_command)
								.hint_text("xxx.sh \"{file}\" {line}")
							);
						});
						ui.weak("Example: gvim --remote-silent \"{file}\":{line}\n\
										Use {file} for the source file and {line} for the line number.");
						ui.add_space(15.0);

						// 4. 自动 DPI 开关
						let mut auto_dpi = self.config.auto_crop_dpi;
						if ui.checkbox(&mut auto_dpi, "Enable Auto Crop DPI (Vector vs Raster)").changed() {
							self.config.auto_crop_dpi = auto_dpi;
							self.enable_auto_crop_dpi = auto_dpi;
						}

						ui.add_space(20.0);
						ui.separator();
						ui.add_space(10.0);

						// 5. OCR 模型管理
						ui.heading("🤖 OCR Models");
						ui.add_space(8.0);

						let mut model_to_remove = None;
						for (idx, model) in self.config.ocr_models.iter().enumerate() {
							ui.group(|ui| {
								ui.horizontal(|ui| {
									ui.strong(&model.name);
									ui.weak(format!("({} / {})", model.provider, model.model_id));
									ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
										if ui.button("🗑 Delete").clicked() {
											model_to_remove = Some(idx);
										}
									});
								});
								ui.label(format!("API URL: {}", model.api_url));
							});
							ui.add_space(4.0);
						}

						// 执行删除
						if let Some(idx) = model_to_remove {
							self.config.ocr_models.remove(idx);
						}

						ui.add_space(10.0);
						ui.strong("➕ Add New Model");
						ui.add_space(5.0);

						ui.horizontal(|ui| {
							ui.label("Name:");
							ui.text_edit_singleline(&mut self.new_ocr_name);
						});
						ui.horizontal(|ui| {
							ui.label("Model ID:");
							ui.text_edit_singleline(&mut self.new_ocr_model_id);
						});
						ui.horizontal(|ui| {
							ui.label("Provider:");
							ui.text_edit_singleline(&mut self.new_ocr_provider);
						});
						ui.horizontal(|ui| {
							ui.label("API URL:");
							ui.text_edit_singleline(&mut self.new_ocr_api_url);
						});
						ui.horizontal(|ui| {
							ui.label("API Token:");
							ui.text_edit_singleline(&mut self.new_ocr_api_token);
						});

						ui.add_space(5.0);
						if ui.button("Add to List").clicked() {
							if !self.new_ocr_name.is_empty() && !self.new_ocr_model_id.is_empty() {
								self.config.ocr_models.push(OcrModelConfig {
									name: self.new_ocr_name.clone(),
									model_id: self.new_ocr_model_id.clone(),
									provider: if self.new_ocr_provider.is_empty() { "local".to_string() } else { self.new_ocr_provider.clone() },
									api_token: self.new_ocr_api_token.clone(),
									api_url: self.new_ocr_api_url.clone(),
								});
								// 清空输入框以便继续添加
								self.new_ocr_name.clear();
								self.new_ocr_model_id.clear();
								self.new_ocr_provider.clear();
								self.new_ocr_api_url.clear();
								self.new_ocr_api_token.clear();
							}
						}

						ui.add_space(20.0);
					});
				ui.separator();
				ui.add_space(10.0);

				// 底部操作按钮
				ui.horizontal(|ui| {
					if ui.button("Save").clicked() {
						self.save_all_config();
						should_close = true;
					}

					ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
						if ui.button("Cancel").clicked() {
							should_cancel = true;
						}
					});
				});
			});

		if should_close {
			is_open = false;
		}
		if should_cancel {
			self.config = load_config(); // 恢复配置
			is_open = false;
		}

		self.show_settings_window = is_open;
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
		if !self.config.show_favorites {return;}

		let mut to_open: Option<PathBuf> = None;
		let mut file_to_remove: Option<usize> = None;
		let mut folder_to_remove: Option<usize> = None;
		let mut move_file: Option<(usize, isize)> = None;   // (索引, 方向: -1上, 1下)
		let mut move_folder: Option<(usize, isize)> = None;

		ui.vertical_centered(|ui| {
			ui.heading("Favorites");
			ui.add_space(20.0);
			// --- 切换开关 ---
			let target_value = !self.config.show_full_path;
			let toggle_label = if self.config.show_full_path { 
				"Display Mode: Full Path" 
			} else { 
				"Display Mode: File Name Only"
			};

			// 参数：可变引用, 选中时赋予的值, 文本标签
			if ui.selectable_value(&mut self.config.show_full_path, target_value, toggle_label)
				.on_hover_text("Click to toggle display mode").changed() 
			{
				self.save_all_config();
			}
			ui.add_space(10.0);
			// ----------------------

			egui::ScrollArea::vertical()
				.max_height(400.0) // Optional: Cap the height so it doesn't take the whole screen
				.auto_shrink([false, true]) 
				.show(ui, |ui| {
					// Ensure buttons stay centered inside the scroll area
					ui.vertical_centered(|ui| {
						let total_files = self.config.favorite_files.len();
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
								//if idx > 0 && ui.button("⬆ Move Up").clicked() {
								//	move_file = Some((idx, -1));
								//	ui.close_kind(egui::UiKind::Menu);
								//}
								//if idx < self.config.favorite_files.len() - 1 && ui.button("⬇ Move Down").clicked() {
								//	move_file = Some((idx, 1));
								//	ui.close_kind(egui::UiKind::Menu);
								//}
								if idx > 0 {
									if ui.button("⏫ Move to Top").clicked() {
										move_file = Some((idx, -2));
										ui.close_kind(egui::UiKind::Menu);
									}
									if ui.button("⬆ Move Up").clicked() {
										move_file = Some((idx, -1));
										ui.close_kind(egui::UiKind::Menu);
									}
								}
								if idx < total_files - 1 {
									if ui.button("⬇ Move Down").clicked() {
										move_file = Some((idx, 1));
										ui.close_kind(egui::UiKind::Menu);
									}
									if ui.button("⏬ Move to Bottom").clicked() {
										move_file = Some((idx, 2));
										ui.close_kind(egui::UiKind::Menu);
									}
								}

								ui.separator();

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
									spawn_and_reap(Command::new("xdg-open") .arg(folder) .spawn());
									//std::process::Command::new("xdg-open").arg(folder).spawn().ok();

									#[cfg(target_os = "windows")]
									spawn_and_reap(Command::new("explorer").arg(folder).spawn());
									//std::process::Command::new("explorer").arg(folder).spawn().ok();

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
						let total_folders = self.config.favorite_folders.len();
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
									//if idx > 0 && ui.button("⬆ Move Up").clicked() {
									//	move_folder = Some((idx, -1));
									//	ui.close_kind(egui::UiKind::Menu);
									//}
									//if idx < self.config.favorite_folders.len() - 1 && ui.button("⬇ Move Down").clicked() {
									//	move_folder = Some((idx, 1));
									//	ui.close_kind(egui::UiKind::Menu);
									//}
									if idx > 0 {
										if ui.button("⏫Move to Top").clicked() {
											move_folder = Some((idx, -2));
											ui.close_kind(egui::UiKind::Menu);
										}
										if ui.button("⬆ Move Up").clicked() {
											move_folder = Some((idx, -1));
											ui.close_kind(egui::UiKind::Menu);
										}
									}
									if idx < total_folders - 1 {
										if ui.button("⬇ Move Down").clicked() {
											move_folder = Some((idx, 1));
											ui.close_kind(egui::UiKind::Menu);
										}
										if ui.button("⏬Move to Bottom").clicked() {
											move_folder = Some((idx, 2));
											ui.close_kind(egui::UiKind::Menu);
										}
									}

									ui.separator();
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
            //let target = if dir == -1 { idx - 1 } else { idx + 1 };
            //self.config.favorite_files.swap(idx, target);
					match dir {
						-2 => { // 置顶：移到索引 0
							let item = self.config.favorite_files.remove(idx);
							self.config.favorite_files.insert(0, item);
						}
						2 => { // 置底：移到最后
							let item = self.config.favorite_files.remove(idx);
							self.config.favorite_files.push(item);
						}
						-1 => self.config.favorite_files.swap(idx, idx - 1), // 正常上移
						1 => self.config.favorite_files.swap(idx, idx + 1),  // 正常下移
						_ => {}
						}
            self.save_all_config();
        }

        // 处理文件夹操作
        if let Some(idx) = folder_to_remove {
            self.config.favorite_folders.remove(idx);
            self.save_all_config();
        }
        if let Some((idx, dir)) = move_folder {
            //let target = if dir == -1 { idx - 1 } else { idx + 1 };
            //self.config.favorite_folders.swap(idx, target);
					match dir {
						-2 => { // 置顶
							let item = self.config.favorite_folders.remove(idx);
							self.config.favorite_folders.insert(0, item);
						}
						2 => { // 置底
							let item = self.config.favorite_folders.remove(idx);
							self.config.favorite_folders.push(item);
						}
						-1 => self.config.favorite_folders.swap(idx, idx - 1),
						1 => self.config.favorite_folders.swap(idx, idx + 1),
						_ => {}
						}
            self.save_all_config();
        }
	}
}
// 统一保存
impl PdfApp {
    fn save_all_config(&self) {
				save_config(&self.config);
    }
}
//加载pdf
impl PdfApp {
	fn load_pdf_path(&mut self, path: std::path::PathBuf) -> bool {
		let already_registered = self.ipc_handle.is_some();
		if !already_registered && !self.try_register_ipc(&path) {
			return false;
		}
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

					//// 记忆新文件的文件夹
					//if let Some(parent) = path.parent() {
					//	self.last_opened_dir = Some(parent.to_path_buf());
					//}
					//// 记忆文件名
					//if let Some(os_str) = path.file_stem() {
					//	self.pdf_name = Some(os_str.to_string_lossy().to_string());
					//}

					// 从历史记录恢复页码
					self.current_page = *self.history.last_pages.get(&path_str).unwrap_or(&0);

					// 启动新文件的文件监控（针对全新打开的文件）
					if let Ok(full_path) = path.canonicalize() {
						self.setup_watcher(full_path);
					}
				}

				// 记忆新文件的文件夹
				if let Some(parent) = path.parent() {
					self.last_opened_dir = Some(parent.to_path_buf());
				}
				// 记忆文件名
				if let Some(os_str) = path.file_stem() {
					self.pdf_name = Some(os_str.to_string_lossy().to_string());
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

				//if let Some(doc_ref) = &self.pdf_doc {
				//	if let Ok(page) = doc_ref.pages().get(self.current_page as u16) {
				//		self.target_dpi = suggest_page_dpi(&page);
				//	}
				//}

				true // 返回加载成功
			}
			Err(_e) => {
				false 
			}
		}
	}
}
//PDF文件拖拽
impl PdfApp {
    fn handle_main_window_drag_drop(&mut self, ctx: &egui::Context) {
        // 合并窗口打开时，不处理主窗口拖拽
        if self.show_merge_window {
            return;
        }

        // 处理拖入的 PDF 文件
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                for file in &i.raw.dropped_files {
                    if let Some(path) = &file.path {
                        let is_pdf = path.extension()
                            .and_then(|s| s.to_str())
                            .map(|s| s.eq_ignore_ascii_case("pdf"))
                            .unwrap_or(false);

                        if is_pdf {
                            self.load_pdf_path(path.clone());
                            break;
                        }
                    }
                }
            }
        });

        // 拖拽悬停提示
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            egui::Area::new(egui::Id::new("drop_target"))
                .anchor(
                    egui::Align2::CENTER_CENTER,
                    egui::vec2(0.0, 0.0)
                )
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new("Drop PDF here to open")
                            .size(32.0)
                            .color(egui::Color32::LIGHT_BLUE)
                            .background_color(
                                egui::Color32::from_black_alpha(150)
                            )
                    );
                });
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

		// 根据当前页面是否包含矢量对象设置截图 DPI
		if self.enable_auto_crop_dpi {
			self.target_dpi = if check_page_has_vector(&page) { 400.0 } else { 200.0 };
		}

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
// 检测页面是否包含矢量对象
fn check_page_has_vector(
    page: &pdfium_render::prelude::PdfPage,
) -> bool {
    let objects = page.objects();

    for obj in objects.iter() {
        match obj.object_type() {
            PdfPageObjectType::Path
            | PdfPageObjectType::Text => return true,
            _ => {}
        }
    }

    false
}

// 尝试为当前要加载的 PDF 注册/连接 IPC 管道
impl PdfApp {
    fn try_register_ipc(&mut self, path: &std::path::Path) -> bool {
        let path_str = path.to_string_lossy().to_string();
        
        // 如果当前已经在看这个文件，直接通过
        if let Some(current_p) = &self.pdf_path {
            if current_p.to_string_lossy() == path_str {
                return true;
            }
        }

        let hash = pdf_ipc_hash(path);
        let ipc_message = IpcMessage {
            path: Some(path.to_path_buf()), 
            //page: Some(self.current_page.max(1)),
            page: Some(self.current_page),
            servername: self.vim_servername.clone(),
            editor: self.editor.clone(),
						synctex: None,
        };

        match setup_ipc(hash, ipc_message) {
            IpcStartup::Exit => {
                // 如果是冷启动时加载冲突，直接退出进程避免多余窗口
                #[cfg(not(target_os = "windows"))]
                if self.pdf_path.is_none() {
                    std::process::exit(0);
                }
                false
            }
						IpcStartup::Primary(rx, handle) => {
							self.ipc_receiver = Some(rx);
							self.ipc_handle = Some(handle);
							true
						}
        }
    }
}

impl PdfApp {
    // 退出时清理当前 PDF 对应的 IPC Socket 文件（仅限非 Windows 平台）
    #[cfg(not(target_os = "windows"))]
    fn cleanup_ipc_socket(&self) {
        if let Some(path) = &self.pdf_path {
            let hash = pdf_ipc_hash(path);
            let socket_path = format!("/tmp/vectorsnap_{:016x}.sock", hash);
            let _ = std::fs::remove_file(socket_path);
        }
    }
}
// 优先解析 synctex,失败则退回直接指定的 page

impl PdfApp {
fn resolve_target_page(&self, page: Option<usize>, synctex: Option<&str>) -> Option<usize> {
    if let (Some(query), Some(pdf_path)) = (synctex, &self.pdf_path) {
        if let Some(p) = crate::synctex::resolve_synctex_page(pdf_path, query) {
            return Some(p);
        }
    }
    page
}
}
impl PdfApp {
fn process_ipc_messages(&mut self, ctx: &egui::Context) {
    // 1. 先取出所有待处理消息，避免借用冲突
    let mut ipc_messages = Vec::new();

    if let Some(rx) = &self.ipc_receiver {
        while let Ok(msg) = rx.try_recv() {
            ipc_messages.push(msg);
        }
    }

    // 2. 处理消息
    for msg in ipc_messages {
        // 更新 Vim ServerName
        if let Some(name) = msg.servername {
            self.vim_servername = if name.trim().is_empty() {
                None
            } else {
                Some(name)
            };
        }

        // 更新编辑器
        if let Some(editor) = msg.editor {
            self.editor = if editor.trim().is_empty() {
                None
            } else {
                Some(editor)
            };
        }

				// 更新 Synctex Path
				if let Some(sync) = &msg.synctex {
					let sync = sync.trim();

					self.synctex = if sync.is_empty() {
						None
					} else {
						Some(sync.to_string())
					};
				}

        // 加载 PDF
        if let Some(new_path) = msg.path {
            self.load_pdf_path(new_path);
        }

				// 优先 synctex,其次直接指定的 page
        let target_page = self.resolve_target_page(msg.page, msg.synctex.as_deref());
        // 跳转到指定页码
        if let Some(p) = target_page {
            if let Some(doc) = &self.pdf_doc {
                if p < doc.pages().len() as usize {
                    self.current_page = p;
                    self.texture = None;
                }
            }
        }

        //// 跳转到指定页码
        //if let Some(p) = msg.page {
        //    if let Some(doc) = &self.pdf_doc {
        //        if p < doc.pages().len() as usize {
        //            self.current_page = p;
        //            self.texture = None;
        //        }
        //    }
        //}

        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }
}
}
//主循环
impl eframe::App for PdfApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		// 轮询后台 IPC 管道，响应来自 SyncTeX 的跳转与唤醒请求
		self.process_ipc_messages(ctx);

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
		// 主窗口文件拖拽处理
		self.handle_main_window_drag_drop(ctx);
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

		// 调用设置窗口
		if self.show_settings_window {
			self.render_settings_window(ctx);
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

		#[cfg(not(target_os = "windows"))]
		self.cleanup_ipc_socket();
	}
}

//main函数
fn main() -> eframe::Result<()> {
    // 1. Extract command line arguments
    let args: Vec<String> = std::env::args().collect();
		let (target_path, target_page, target_servername, target_editor, target_synctex) = parse_args(&args);

		let ipc_message = IpcMessage {
        path: target_path.clone(),
        page: target_page,
        servername: target_servername.clone(),
        editor: target_editor.clone(),
				synctex: target_synctex.clone(),
    };

		let (ipc_receiver, ipc_handle) = if let Some(pdf_path) = target_path.as_ref() {
        let hash = pdf_ipc_hash(pdf_path);
        match setup_ipc(hash, ipc_message) {
            IpcStartup::Exit => { return Ok(()); }

            IpcStartup::Primary(rx, handle) => { (Some(rx), Some(handle)) }
        }
    } else {
        // 没有 PDF 路径：直接创建独立空白窗口
        (None, None)
    };

    // --- Rest of your existing window, font, and eframe run_native setup ---
    let icon_bytes = include_bytes!("../assets/pdf-reader.png");
    let window_icon = match image::load_from_memory(icon_bytes) {
        Ok(img) => {
            let rgba = img.into_rgba8();
            let (width, height) = rgba.dimensions();
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            })
        }
        _ => None,
    };
    let viewport = egui::ViewportBuilder::default().with_inner_size([900.0, 1000.0]);
    let viewport = if let Some(icon) = window_icon {
        viewport.with_icon(icon)
    } else {
        viewport
    };
    let options = eframe::NativeOptions {
        viewport,
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
                let target_height = 1020.0f32.min(monitor_size.y - 80.0);
                cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                    egui::vec2(800.0, target_height),
                ));
                cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                    egui::pos2(100.0, 0.0),
                ));
            }
            setup_fonts(&cc.egui_ctx);

            // Pass rx receiver into PdfApp
						let mut app = PdfApp::new(cc, target_path, ipc_receiver, ipc_handle);
						app.vim_servername = target_servername;
						app.editor = target_editor;
						app.synctex = target_synctex.clone();

            //if let Some(p) = target_page {
            //    app.current_page = p;
            //    app.texture = None;
            //}
						let startup_page = app.resolve_target_page(target_page, target_synctex.as_deref());
						if let Some(p) = startup_page {
							if let Some(doc) = &app.pdf_doc {
								let total = doc.pages().len() as usize;
								app.current_page = p.min(total.saturating_sub(1));
								app.texture = None;
							}
						}
            Ok(Box::new(app))
        }),
    )
}
