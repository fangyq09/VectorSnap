use std::sync::Arc;
use base64::Engine;


use crate::{OcrModelConfig, PdfApp};

use crate::latex_preview::normalize_latex;


//OCR LaTeX
impl PdfApp {
	pub fn render_ocr_result_window(&mut self, ctx: &egui::Context) {
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
pub fn run_latex_ocr(&self, img: image::RgbaImage, ctx: egui::Context) {
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
pub fn execute_vision_task_cloud(&self, img: image::RgbaImage, ctx: egui::Context, prompt: &'static str) {
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
	pub fn execute_vision_task_local(&self, img: image::RgbaImage, ctx: egui::Context, prompt: &'static str) {
		// 1. 状态检查
		if *self.is_ocr_loading.lock().unwrap() {
			return;
		}

		// 取当前选中模型的完整配置（含 llama-mtmd-cli / gguf / mmproj 路径）
		// 找不到时退回旧行为：仅用 model_id 按默认目录查找
		let model_cfg = self.config.ocr_models.iter()
			.find(|m| m.name == self.config.current_ocr_model_name)
			.cloned()
			.unwrap_or_else(|| OcrModelConfig {
				model_id: self.ocr_model_name.clone(),
				provider: "local".to_string(),
				..Default::default()
			});

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
			let result = (|| -> Result<String, String> {
				// --- A. 图像预处理与临时保存 ---
				let img_to_send = scale_image_for_ocr(img);

				// 保存到临时文件供外部 EXE 读取
				let temp_path = std::env::temp_dir().join("ocr_local_input.png");
				img_to_send.save(&temp_path)
					.map_err(|e| format!("无法保存临时图片: {}", e))?;
				let img_path_str = temp_path.to_string_lossy().to_string();

				// --- B. 获取本地路径与执行 ---
				let (exe, model, mmproj) = get_ocr_paths(&model_cfg)?;

				let output = std::process::Command::new(&exe)
					.arg("-m").arg(&model)
					.arg("--jinja")
					.arg("--mmproj").arg(&mmproj)
					.arg("--image").arg(&img_path_str)
					.arg("--temp").arg("0.1")
					.arg("-p").arg(prompt)
					.arg("-n").arg("2048")
					.arg("-c").arg("16384")
					.output()
					.map_err(|e| format!("无法启动 {}: {}", exe.display(), e))?;

				if !output.status.success() {
					// 取 stderr 最后几行，方便定位问题
					let err = String::from_utf8_lossy(&output.stderr);
					let mut tail: Vec<&str> = err.lines().rev().take(5).collect();
					tail.reverse();
					return Err(format!("退出码 {:?}\n{}", output.status.code(), tail.join("\n")));
				}

				// --- C. 清洗输出 ---
				let raw = String::from_utf8_lossy(&output.stdout);
				let cleaned = raw.lines()
					.filter(|line| !line.is_empty() && !line.starts_with("llama_"))
					.collect::<Vec<_>>()
					.join("\n");
				Ok(cleaned.trim().to_string())
			})();

			// --- D. 结果回传 ---
			{
				let mut res_lock = ocr_res.lock().unwrap();
				let mut loading_lock = loading.lock().unwrap();

				*res_lock = Some(match result {
					Ok(text) => text,
					Err(e) => format!("OCR 运行失败: {}", e),
				});
				*loading_lock = false;
			}
			ctx_clone.request_repaint();
		});
	}
}

// 去掉首尾空白与 Windows “复制为路径” 带来的引号
fn clean_path(s: &str) -> &str {
	s.trim().trim_matches('"')
}

// 解析 (llama-mtmd-cli, gguf, mmproj) 三个路径。
// 配置里填了就用配置；没填则回退到旧的默认规则，保证老配置照常工作。
fn get_ocr_paths(cfg: &OcrModelConfig) -> Result<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf), String> {
	use std::path::PathBuf;

	// 1. llama-mtmd-cli
	let exe_path = if !clean_path(&cfg.exe_path).is_empty() {
		PathBuf::from(clean_path(&cfg.exe_path))
	} else {
		let exe_name = if cfg!(target_os = "windows") { "llama-mtmd-cli.exe" } else { "llama-mtmd-cli" };
		let local_bin = dirs::home_dir().map(|h| h.join(".local/bin").join(exe_name));
		match local_bin {
			Some(p) if p.exists() => p,
			_ => std::env::current_exe().ok()
				.and_then(|e| e.parent().map(|d| d.join(exe_name)))
				.ok_or_else(|| "无法定位 llama-mtmd-cli，请在设置中指定路径".to_string())?,
		}
	};
	if !exe_path.is_file() {
		return Err(format!("找不到 llama-mtmd-cli: {}", exe_path.display()));
	}

	// 2. gguf 与 mmproj
	let default_dir = || dirs::data_local_dir().map(|d| d.join("llama_models"));

	let model_path = if !clean_path(&cfg.model_path).is_empty() {
		PathBuf::from(clean_path(&cfg.model_path))
	} else {
		default_dir()
			.ok_or_else(|| "无法定位默认模型目录".to_string())?
			.join(format!("{}.gguf", cfg.model_id))
	};
	if !model_path.is_file() {
		return Err(format!("找不到 gguf 模型文件: {}", model_path.display()));
	}

	let mmproj_path = if !clean_path(&cfg.mmproj_path).is_empty() {
		PathBuf::from(clean_path(&cfg.mmproj_path))
	} else {
		default_dir()
			.ok_or_else(|| "无法定位默认模型目录".to_string())?
			.join(format!("mmproj-{}.gguf", cfg.model_id))
	};
	if !mmproj_path.is_file() {
		return Err(format!("找不到 mmproj 文件: {}", mmproj_path.display()));
	}

	Ok((exe_path, model_path, mmproj_path))
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

