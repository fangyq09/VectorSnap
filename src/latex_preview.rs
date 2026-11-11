use std::sync::Arc;
use std::fs;

use pdfium_render::prelude::{PdfColor, PdfRenderConfig}; 

use crate::PdfApp;
use crate::copy_image_to_clipboard;
use crate::get_config_dir;


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
pub fn render_latex_preview_window(&mut self, ctx: &egui::Context) {
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
pub fn solve_latex_preview(&self, latex_query: String, ctx: egui::Context) {
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
pub fn normalize_latex(content: &str) -> String {
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
		// 从 config 目录读取 template.tex
		//模板中包含约定的占位符（{{content}}），将片段替换进去
		let template_path = get_config_dir().join("template.tex");

		if let Ok(template_content) = std::fs::read_to_string(&template_path) {
			if template_content.contains("{{content}}") {
				return template_content.replace("{{content}}", trimmed);
			}
		}

    // 兜底默认模板，格式：包装成标准 standalone 格式并返回
		// 这样用户在 UI 里就能看到完整的代码结构，方便他们添加其他宏包
		let preamble = r#"\documentclass[border=2pt, preview]{standalone}
		\usepackage{amsmath, amsfonts, amssymb, bm, enumitem, varwidth}
		\setenumerate[1]{itemsep=5pt,partopsep=0pt,parsep=5pt,topsep=5pt}
		\setitemize[1]{itemsep=5pt,partopsep=0pt,parsep=5pt,topsep=5pt, leftmargin=2pc}
		\begin{document}
		\begin{varwidth}{\linewidth}"#;

		let postamble = r#"\end{varwidth}
		\end{document}"#;

		format!("{}\n\n{}\n\n{}", preamble, trimmed, postamble)
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
