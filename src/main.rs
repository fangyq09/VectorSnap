#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use pdfium_render::prelude::*;
use std::path::PathBuf;
use std::collections::HashSet;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct AppConfig {
    pub favorite_files: Vec<std::path::PathBuf>,
    pub favorite_folders: Vec<std::path::PathBuf>,

		#[serde(default)] 
    pub show_full_path: bool,
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
}
struct CachedLink {
    rect: egui::Rect,
    destination: Option<usize>, 
    uri: Option<String>,       
}
//字段
struct PdfApp {
    //pdfium: Pdfium,
		pdfium: &'static Pdfium,
    pdf_doc: Option<PdfDocument<'static>>,
    current_page: usize,
    texture: Option<egui::TextureHandle>,
		cropped_tex: Option<egui::TextureHandle>, 
		last_cropped_image: Option<image::RgbaImage>,
		pdf_name: Option<String>,
		pdf_path: Option<PathBuf>,
		target_dpi: f32,
		view_scale: f32,
		zoom_factor: f32,
		last_opened_dir: Option<std::path::PathBuf>,
		drag_start_local: Option<egui::Pos2>,     
    selection_rect_local: Option<egui::Rect>,
		favorite_files: Vec<PathBuf>,
		favorite_folders: Vec<PathBuf>,
		show_full_path: bool,
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
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 加载文泉驿
    fonts.font_data.insert(
        "wqy".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/wqy-microhei.ttc")).into(),
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
// 根据操作系统嵌入不同的库文件
#[cfg(target_os = "linux")]
const PDFIUM_LIB_BYTES: &[u8] = include_bytes!("../assets/libpdfium.so");

#[cfg(target_os = "windows")]
const PDFIUM_LIB_BYTES: &[u8] = include_bytes!("../assets/pdfium.dll");

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
	pub fn new(cc: &eframe::CreationContext<'_>, path: Option<std::path::PathBuf>) -> Self {
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

		// --- 获取屏幕密度并计算 view_scale ---
		let current_ppp = cc.egui_ctx.pixels_per_point();
		let ideal_scale = Self::calculate_ideal_view_scale(current_ppp);

		let config = load_config();

		let mut app = Self {
			pdfium,
			pdf_doc: None,
			current_page: 0,
			texture: None,
			cropped_tex: None,
			last_cropped_image: None,
			pdf_name: None,
			pdf_path: None,
			target_dpi: 600.0,
			//view_scale: 1.5,
			view_scale: ideal_scale,
			zoom_factor: 1.0,
			last_opened_dir: None,
			drag_start_local: None,
			selection_rect_local: None,
			favorite_files: config.favorite_files,
			favorite_folders: config.favorite_folders,
			show_full_path: config.show_full_path,
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
		};

		// 如果启动参数里有路径，直接调用加载函数
		if let Some(p) = path {
			app.load_pdf_path(p);
		}

		app
	}
}
impl PdfApp {
    // 根据当前屏幕密度（ppp）计算理想的渲染倍率（view_scale）
    // 目标是让（ppp * view_scale）的总物理倍率保持在 2.0 左右
    fn calculate_ideal_view_scale(ppp: f32) -> f32 {
        if ppp <= 1.1 {
            // 96 DPI 左右的普通屏，1.0x 渲染太虚，强力开启 2.0x 超采样
            2.0
        } else if ppp <= 1.6 {
            // 125% - 150% 缩放的屏幕，1.5x 渲染足够平滑
            1.5
        } else if ppp < 2.0 {
            // Retina 或 200% 缩放的高分屏，物理像素已翻倍，1.25x 即可达到极致清晰
            1.25
        } else {
            // 4K 极高密度屏，直接 1.0x 渲染，节省内存和渲染开销
            1.0
        }
    }
}
//主循环
impl eframe::App for PdfApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {

		// 发送命令给窗口管理器更新标题
		let title = self.pdf_name.as_deref().unwrap_or("VectorSnap").to_string();
		ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

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
	}
}
//快捷键
impl PdfApp {
	fn handle_shortcuts(&mut self, ctx: &egui::Context) {
		let mut action = ShortcutAction::None;

		// 避免在输入框中误触发快捷键，但允许 Ctrl 组合键
		if ctx.wants_keyboard_input() && !ctx.input(|i| i.modifiers.ctrl) {
			return;
		}

		ctx.input_mut(|i| {
			let up_10 = i.consume_key(egui::Modifiers::SHIFT, egui::Key::K);
			let down_10 = i.consume_key(egui::Modifiers::SHIFT, egui::Key::J);

			let up = i.consume_key(egui::Modifiers::NONE, egui::Key::K)
				|| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);

			let down = i.consume_key(egui::Modifiers::NONE, egui::Key::J)
				|| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);

			let home = i.consume_key(egui::Modifiers::NONE, egui::Key::Num0)
				|| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Num6) 
				|| i.consume_key(egui::Modifiers::NONE, egui::Key::Home);

			let end = i.consume_key(egui::Modifiers::SHIFT, egui::Key::Num4)
				|| i.consume_key(egui::Modifiers::SHIFT, egui::Key::G)
				|| i.consume_key(egui::Modifiers::NONE, egui::Key::End);


			//if up { action = ShortcutAction::PrevPage; }
			//if down { action = ShortcutAction::NextPage; }
			//if home { action = ShortcutAction::FirstPage; }
			//if end { action = ShortcutAction::LastPage; }
			//if up_10 { action = ShortcutAction::Prev10Pages; }
			//if down_10 { action = ShortcutAction::Next10Pages; }
			if up_10 { 
				action = ShortcutAction::Prev10Pages; 
			} else if down_10 { 
				action = ShortcutAction::Next10Pages; 
			} else if up { 
				action = ShortcutAction::PrevPage; 
			} else if down { 
				action = ShortcutAction::NextPage; 
			} else if home { 
				action = ShortcutAction::FirstPage; 
			} else if end { 
				action = ShortcutAction::LastPage; 
			}

			// 拦截 Ctrl + '=' 或 Ctrl + '+'
			if i.consume_key(egui::Modifiers::CTRL, egui::Key::Equals) || 
				i.consume_key(egui::Modifiers::CTRL, egui::Key::Plus) {
					action = ShortcutAction::ZoomIn;
			}

			// 拦截 Ctrl + '-'
			if i.consume_key(egui::Modifiers::CTRL, egui::Key::Minus) {
				action = ShortcutAction::ZoomOut;
			}

			// 拦截 Ctrl + '0'
			if i.consume_key(egui::Modifiers::CTRL, egui::Key::Num0) {
				action = ShortcutAction::ResetZoom;
			}

			if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
				action = ShortcutAction::ClosePreview;
			}

			if i.consume_key(egui::Modifiers::NONE, egui::Key::Q) {
				action = ShortcutAction::ResetApp;
			}

			// Ctrl+C (egui Copy event)
			if i.events.iter().any(|e| matches!(e, egui::Event::Copy)) {
				if self.last_cropped_image.is_some() {
					action = ShortcutAction::CopyImage;
				}
			}

			// Ctrl+S
			if i.modifiers.ctrl && i.key_pressed(egui::Key::S) {
				if self.last_cropped_image.is_some() {
					action = ShortcutAction::SaveImage;
				}
			}
		});

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
				self.cropped_tex = None;
				self.last_cropped_image = None;
			}

			ShortcutAction::CopyImage => {
				self.copy_image_to_clipboard();
				self.cropped_tex = None;
				self.last_cropped_image = None;
			}

			ShortcutAction::SaveImage => {
				self.save_image_with_format("png");
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


						// --- View Scale 下拉菜单 ---
						ui.separator();
						ui.label("View Quality:");
						egui::ComboBox::from_id_salt("view_scale")
							.width(80.0) 
							.selected_text(match self.view_scale {
								//0.8 => "Low",
								1.0 => "Fast",
								1.25 => "Balanced",
								1.5 => "High-Q",
								2.0 => "HiDPI",
								_ => "Custom",
							})
						.show_ui(ui, |ui| {
							// 定义一个闭包来简化代码，因为每个选项被点击后都要清除纹理
							let mut select_scale = |ui: &mut egui::Ui, value: f32, label: &str| {
								if ui.selectable_value(&mut self.view_scale, value, label).clicked() {
									self.texture = None; // 切换倍率必须重绘，否则画面会模糊或锯齿
								}
							};

							//select_scale(ui, 0.8, "Low");
							select_scale(ui, 1.0, "Fast");
							select_scale(ui, 1.25, "Balanced");
							select_scale(ui, 1.5, "High-Q");
							select_scale(ui, 2.0, "HiDPI");
						});

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

						ui.separator();
						ui.menu_button("More", |ui| {
							if ui.button("Export Pages").clicked() {
								self.show_export_window = true;
								ui.close_kind(egui::UiKind::Menu);
							}

							ui.separator();

							//if ui.button("Print").clicked() {
							//	if let Some(path) = &self.pdf_path { 
							//		//self.print_document(path);
							//	}
							//	ui.close_kind(egui::UiKind::Menu);
							//}

							//ui.separator();

							if ui.button("Rotate CW 90°").clicked() {
								self.rotate_current_page(90.0);
							}
							if ui.button("Rotate CCW 90°").clicked() {
								self.rotate_current_page(-90.0);
							}
							ui.separator();
							if ui.button("🔍 Zoom In (+)").clicked() {
								// 限制最大放大到 10 倍
								self.zoom_factor = (self.zoom_factor * 1.1).clamp(0.1, 10.0);
								//self.texture = None;
							}
							if ui.button("🔍 Zoom Out (-)").clicked() {
								// 限制最小缩小到 0.1 倍
								self.zoom_factor = (self.zoom_factor / 1.1).clamp(0.1, 10.0);
								//self.texture = None;
							}
							if ui.button("🔄 Reset Zoom").clicked() {
								self.zoom_factor = 1.0;
								//self.texture = None;
							}

							ui.separator();
							if ui.button("Help & Shortcuts").clicked() {
								self.show_help_window = true;
								//ui.close_menu();
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
			let toggle_label = if self.show_full_path { "Display Mode: Full Path" } else { "Display Mode: File Name Only" };
			if ui.button(toggle_label).on_hover_text("Click to toggle between full file path and name only").clicked() {
				self.show_full_path = !self.show_full_path;
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
						for (idx, path) in self.favorite_files.iter().enumerate() {
							let label = if self.show_full_path {
								path.display().to_string()
							} else {
								format!("📄 {}", path.file_name().unwrap_or_default().to_string_lossy())
							};

							let response = ui.button(label);

							if !self.show_full_path {
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
								if idx < self.favorite_files.len() - 1 && ui.button("⬇ Move Down").clicked() {
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
					if !self.favorite_files.contains(&path) {
						self.favorite_files.push(path);
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
						for (idx, path) in self.favorite_folders.iter().enumerate() {
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
									if idx < self.favorite_folders.len() - 1 && ui.button("⬇ Move Down").clicked() {
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
					if !self.favorite_folders.contains(&path) {
						self.favorite_folders.push(path);
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
            self.favorite_files.remove(idx);
            self.save_all_config();
        }
        if let Some((idx, dir)) = move_file {
            let target = if dir == -1 { idx - 1 } else { idx + 1 };
            self.favorite_files.swap(idx, target);
            self.save_all_config();
        }

        // 处理文件夹操作
        if let Some(idx) = folder_to_remove {
            self.favorite_folders.remove(idx);
            self.save_all_config();
        }
        if let Some((idx, dir)) = move_folder {
            let target = if dir == -1 { idx - 1 } else { idx + 1 };
            self.favorite_folders.swap(idx, target);
            self.save_all_config();
        }
	}
// 统一保存
    fn save_all_config(&self) {
        save_config(&AppConfig {
            favorite_files: self.favorite_files.clone(),
            favorite_folders: self.favorite_folders.clone(),
						show_full_path: self.show_full_path,
        });
    }
}
//加载pdf
impl PdfApp {
    fn load_pdf_path(&mut self, path: std::path::PathBuf) {
        // 1. 记忆文件夹
        if let Some(parent) = path.parent() {
            self.last_opened_dir = Some(parent.to_path_buf());
        }
        // 2. 记忆文件名
        if let Some(os_str) = path.file_stem() {
            self.pdf_name = Some(os_str.to_string_lossy().to_string());
        }
        // 3. 加载文档
        let path_str = path.display().to_string();
				if let Ok(doc) = self.pdfium.load_pdf_from_file(&path_str, None) {
					self.pdf_doc = Some(doc);
					self.pdf_path = Some(path.clone());
					self.rotations = vec![0.0; self.pdf_doc.as_ref().unwrap().pages().len().into()];
					self.current_page = 0;
					self.texture = None;
					println!("loaded: {}", path_str);
				}
    }
}
//Reset
impl PdfApp {
    fn unload_pdf(&mut self) {
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
		// UI布局宽度
		let available_rect = ui.available_rect_before_wrap();
		let base_width = available_rect.width();

		let ideal_screen_px = base_width * self.zoom_factor * ppp;
		// 像素对齐（非常关键）
		let screen_px_width = ideal_screen_px.round();

		//显示尺寸
		let display_width = screen_px_width / ppp; 
		let display_height = display_width / aspect_ratio;
		let display_size = egui::vec2(display_width, display_height);

		// ---------- 6. Oversample ----------
		let mut target_px_width = (screen_px_width * self.view_scale).round() as u32;

		const MAX_TEXTURE_SIZE: u32 = 8192;

		target_px_width = target_px_width.min(MAX_TEXTURE_SIZE);

		// ---------- 7. 是否需要重新渲染 ----------
		let needs_re_render = self.texture.as_ref().map_or(true, |tex| {
			let current_width = tex.size()[0] as u32;

			current_width != target_px_width
				|| self.last_rendered_page != self.current_page
				|| self.last_rendered_angle != angle
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
		egui::ScrollArea::both()
			.auto_shrink([false; 2])
			.max_width(base_width)
			.show(ui, |ui| {

				let (mut response, painter) =
					ui.allocate_painter(display_size, egui::Sense::click_and_drag());

				let ppp = painter.pixels_per_point();
				let aligned_min = egui::pos2(
					(response.rect.min.x * ppp).round() / ppp,
					(response.rect.min.y * ppp).round() / ppp,
				);
				response.rect = egui::Rect::from_min_size(aligned_min, display_size);

				if let Some(tex) = &self.texture {

					painter.image(
						tex.id(),
						response.rect,
						egui::Rect::from_min_max(
							egui::pos2(0.0, 0.0),
							egui::pos2(1.0, 1.0),
						),
						egui::Color32::WHITE,
					);
				}

				// 只有换页时才去解析昂贵的 PDF 原始数据
				//if self.last_link_page != Some(self.current_page) {
				//	self.update_link_cache(&page, response.rect, native_width, native_height);
				//	self.last_link_page = Some(self.current_page);
				//}
				if self.last_link_page != Some(self.current_page)
					|| self.last_link_rect != Some(response.rect)
				{
					self.update_link_cache(&page, response.rect, native_width, native_height);
					self.last_link_page = Some(self.current_page);
				}

				if angle == 0.0 {
					if self.handle_link_interaction(ctx, &response, total_pages_count) {
						ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
					}
				}

				self.handle_selection_interaction(&response, &painter, ctx, &page);
			});
			}
		// 核心函数 A：更新缓存（仅在页面/尺寸变化时调用）
		fn update_link_cache(
			&mut self,
			page: &pdfium_render::prelude::PdfPage,
			display_rect: egui::Rect, // 传入对齐后的 response.rect
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
								if let Ok(idx) = dest.page_index() {
									let raw_idx = idx as usize;

									// 1. 获取 Producer 字符串
									let producer = self.pdf_doc.as_ref()
										.and_then(|doc| doc.metadata().get(PdfDocumentMetadataTagType::Producer))
										.map(|tag| tag.value().to_lowercase())
										.unwrap_or_default();

									// 2. pdftex 需要补 1
									let final_idx = {
										let is_tex = producer.contains("pdftex") || producer.contains("latex");
										if is_tex {
											raw_idx + 1
										} else {
											raw_idx
										}
									};

									dest_idx = Some(final_idx);
								}
							} else if let Some(action) = link_data.action() {
								if let Some(uri_action) = action.as_uri_action() {
									uri_str = uri_action.uri().ok();
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
            // 关键：传入 response.rect.size() 以保证比例尺绝对统一
            self.perform_high_res_crop(ctx, page, local_rect, response.rect.size());
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
					let ratio = (700.0f32 / img_size.x).min(500.0f32 / img_size.y).min(1.0f32);

					ui.vertical_centered(|ui| {
						ui.image((tex_id, img_size * ratio)); // 使用提取出的 tex_id
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
							// 第三列：PNG
							cols[2].vertical_centered(|ui| {
								if ui.button(egui::RichText::new("💾 PNG").strong()).clicked() { 
									save_format = Some("png"); 
								}
							});
							// 第四列：JPG
							cols[3].vertical_centered(|ui| {
								if ui.button(egui::RichText::new("🖼 JPG").strong()).clicked() { 
									save_format = Some("jpg"); 
								}
							});
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
			self.copy_image_to_clipboard();
			self.cropped_tex = None;
		}
		if let Some(fmt) = save_format {
			self.save_image_with_format(fmt);
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
                let cropped_sub_image = full_image.view(crop_x, crop_y, crop_w, crop_h).to_image();

                // 纹理更新 (保持原样)
                self.last_cropped_image = Some(cropped_sub_image.clone());
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
fn save_image_with_format(&mut self, extension: &str) {
    if let Some(img_data) = &self.last_cropped_image {
        let base_name = self.pdf_name.as_deref().unwrap_or("Document");
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let default_name = format!("{}_Crop_{}.{}", base_name, timestamp, extension);

        let file = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter(extension, &[extension])
            .save_file();

        if let Some(path) = file {
            // --- 修复逻辑开始 ---
            if extension == "jpg" || extension == "jpeg" {
                // 1. 创建一个同样大小的纯白 RGB 画布
                let mut rgb_img = image::ImageBuffer::new(img_data.width(), img_data.height());
                for (_x, _y, pixel) in rgb_img.enumerate_pixels_mut() {
                    *pixel = image::Rgb([255, 255, 255]); // 填充白色
                }

                // 2. 将 RGBA 图片叠加到白色背景上 (去透明化)
                //use image::GenericImageView;
                for (x, y, rgba) in img_data.enumerate_pixels() {
                    if rgba[3] > 0 { // 如果该像素不是完全透明
                        let rgb = rgb_img.get_pixel_mut(x, y);
                        // 简单的颜色混合（或者直接赋值，因为 PDF 背景通常是白的）
                        *rgb = image::Rgb([rgba[0], rgba[1], rgba[2]]);
                    }
                }
                
                //// 3. 保存 RGB 图片为 JPG
								let file = std::fs::File::create(&path).unwrap();

								// 使用 JpegEncoder 替代简单的 .save()
								// 质量范围 1-100，80 是一个在体积和清晰度之间非常平衡的数值
								let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 80);

								if let Err(e) = encoder.encode_image(&rgb_img) {
									eprintln!("Failed to encode JPG: {}", e);
									return;
								}
            } else {
                // 4. 如果是 PNG，直接保存
                if let Err(e) = img_data.save(&path) {
                    eprintln!("Failed to save PNG: {}", e);
                    return;
                }
            }
            // --- 修复逻辑结束 ---

            println!("Saved successfully: {:?}", path);
            self.cropped_tex = None;
            self.last_cropped_image = None;
        }
    }
}
}
//复制图片
impl PdfApp {
    fn copy_image_to_clipboard(&self) {
        if let Some(img_data) = &self.last_cropped_image {
            let mut clipboard = match arboard::Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init clipboard: {}", e);
                    return;
                }
            };

            // 将 image 库的 RgbaImage 转换为 arboard 的 ImageData
            let image_data = arboard::ImageData {
                width: img_data.width() as usize,
                height: img_data.height() as usize,
                bytes: std::borrow::Cow::Borrowed(img_data.as_raw()),
            };

            if let Err(e) = clipboard.set_image(image_data) {
                eprintln!("Failed to copy image: {}", e);
            } else {
                println!("Image copied to clipboard!");
            }
        }
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

		// --- 修复开始：将 render 逻辑拆开 ---
		let config = pdfium_render::prelude::PdfRenderConfig::new().set_target_width(target_width);

		// 1. 先获取 render 对象
		let render_res = page.render_with_config(&config);

		if let Ok(render) = render_res {
			// 2. 立即转换为图像数据，此时 img_data 是独立的，不再占用 page 的引用
			let img_data = render.as_image(); 

			// 释放对 page 的占用后，再进行后续操作
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

			// 5. 保存到指定路径
			if let Err(e) = new_doc.save_to_file(&path) {
				eprintln!("Failed to save PDF: {}", e);
			} else {
				println!("Successfully extracted page to: {:?}", path);
			}
		}
	}
//另一种不同的逻辑
//fn extract_current_page_pdf(&mut self) {
//    // 1. 确保 PDF 已加载
//    let Some(_doc) = &self.pdf_doc else { return; };
//    let Some(original_path) = &self.pdf_path else { return; };
//
//    // 2. 默认文件名
//    let base_name = self.pdf_name.as_deref().unwrap_or("Document");
//    let page_num = self.current_page + 1;
//    let default_name = format!("{}_Page_{}.pdf", base_name, page_num);
//
//    // 3. 保存对话框
//    if let Some(path) = rfd::FileDialog::new()
//        .set_file_name(&default_name)
//        .add_filter("PDF document", &["pdf"])
//        .save_file()
//    {
//        // 4. 重新加载 PDF
//        let new_doc = match self.pdfium.load_pdf_from_file(original_path, None) {
//            Ok(doc) => doc,
//            Err(e) => {
//                eprintln!("Failed to reload PDF: {}", e);
//                return;
//            }
//        };
//
//        // 5. 获取总页数和当前页（都用 u16）
//        let total: u16 = new_doc.pages().len().into();       // 转成 u16
//				let keep: u16 = self.current_page.try_into().unwrap_or(0);
//        //let keep: u16 = self.current_page;                   // 当前页
//
//        // 6. 倒序删除不需要的页
//        for i in (0..total).rev() {
//            if i != keep {
//                if let Ok(page) = new_doc.pages().get(i) {
//                    let _ = page.delete(); // 忽略 Result
//                }
//            }
//        }
//
//        // 7. 保存
//        if let Err(e) = new_doc.save_to_file(&path) {
//            eprintln!("Failed to save PDF: {}", e);
//        } else {
//            println!("Successfully extracted page to: {:?}", path);
//        }
//    }
//}
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

    let default_name = format!(
        "{}_Selected_Pages.pdf",
        self.pdf_name.as_deref().unwrap_or("Document")
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

        if let Err(e) = new_doc.save_to_file(&path) {
            eprintln!("Failed to save PDF: {}", e);
        }
    }
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
								println!("图片通过 Rust 中转成功: {:?}", path.file_name().unwrap());
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
            .with_inner_size([800.0, 1020.0]),
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
