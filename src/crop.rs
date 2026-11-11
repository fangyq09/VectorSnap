 use pdfium_render::prelude::{PdfPage,PdfPageRenderRotation,PdfRenderConfig}; 

use crate::PdfApp;


//矩形选择区域
impl PdfApp {
pub fn handle_selection_interaction(
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
						self.last_selection_rect = Some(local_rect);
						//if self.enable_auto_crop_dpi {
						//	let has_vector = check_selection_has_vector(page, local_rect, response.rect.size());
						//	self.target_dpi = if has_vector { 400.0 } else { 200.0 };
						//}
						// A. 先执行高分辨率裁剪
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
	pub fn render_preview_window(&mut self, ctx: &egui::Context) {
		// 技巧：先把我们需要的数据“偷”出来，不要在 show 闭包里引用 self.cropped_tex
		let mut should_close = false;
		let mut should_copy = false;
		let mut should_copy_text = false;
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
						ui.columns(5, |cols| {
							// 第一列：Discard
							cols[0].vertical_centered(|ui| {
								if ui.button(egui::RichText::new("🗑 Discard").strong()).clicked() { 
									should_close = true;
								}
							});
							// 第二列：Copy
							cols[1].vertical_centered(|ui| {
								if ui.button(egui::RichText::new("📋 Copy Image").strong()).clicked() {
									should_copy = true; 
								}
							});

							// 第三列：新增的 Copy Text 按钮
							cols[2].vertical_centered(|ui| {
								if ui.button(egui::RichText::new("Copy Text").strong()).clicked() {
									should_copy_text = true;
								}
							});
							
							// 第四列：Save as (合并 PNG 和 JPG)
							cols[3].vertical_centered(|ui| {
								if ui.button(egui::RichText::new("Save as PNG").strong()).clicked() {
										save_format = Some("png");
								}
							});
							//cols[3].vertical_centered(|ui| {
							//	ui.menu_button(egui::RichText::new("Save as").strong(), |ui| {
							//		ui.set_min_width(40.0);
							//		if ui.button("PNG").clicked() {
							//			save_format = Some("png");
							//			ui.close_kind(egui::UiKind::Menu);
							//		}
							//		ui.separator();
							//		if ui.button("JPG").clicked() {
							//			save_format = Some("jpg");
							//			ui.close_kind(egui::UiKind::Menu);
							//		}
							//	});
							//});

							// 第五列：OCR 
							cols[4].vertical_centered(|ui| {
								let btn = ui.button(egui::RichText::new("OCR Text").strong());
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

						});
					});
				});
		}

		// 在 Window 闭包结束后，再去安全地修改 self 状态
		if should_close {
			self.cropped_tex = None;
			self.last_cropped_image = None;
			self.last_selection_rect = None;
		}
		if should_copy {
			if let Some(img) = &self.last_cropped_image {
				copy_image_to_clipboard(img);
			}
			self.cropped_tex = None;
			self.last_selection_rect = None;
		}
		if should_copy_text {
			if let Some(text) = self.extract_text_in_selection() {
				ctx.copy_text(text); 
			}
			self.cropped_tex = None;
			self.last_cropped_image = None;
			self.last_selection_rect = None;
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
pub fn perform_high_res_crop(
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
    pub fn save_generic_image(&mut self, img_to_save: Option<image::RgbaImage>, prefix: &str, extension: &str) {
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
pub fn copy_image_to_clipboard(img: &image::RgbaImage) {
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


//提取文字
impl PdfApp {
    pub fn extract_text_in_selection(&self) -> Option<String> {
        let local_rect = self.last_selection_rect?;
        if self.last_pdf_width <= 0.0 || self.last_pdf_height <= 0.0 {
            return None;
        }
        let actual_ui_size = egui::vec2(self.last_pdf_width, self.last_pdf_height);

        // 1. 获取本地 page 实例
        let doc = self.pdf_doc.as_ref()?;
        let page = doc.pages().get(self.current_page as u16).ok()?;

        // 🌟 解决生命周期的关键：显式绑定 page_text，确保其在 page 之前释放
        let page_text = page.text().ok()?;

        let pdf_width_pts = page.width().value as f32;
        let pdf_height_pts = page.height().value as f32;

        // 1. 缩放比例
        let scale_x = pdf_width_pts / actual_ui_size.x;
        let scale_y = pdf_height_pts / actual_ui_size.y;

        // 2. 换算 Egui 拉框到 PDF 物理坐标（镜像 Y 轴）
        let pdf_min_x = local_rect.min.x * scale_x;
        let pdf_max_x = local_rect.max.x * scale_x;
        let pdf_min_y = pdf_height_pts - (local_rect.max.y * scale_y);
        let pdf_max_y = pdf_height_pts - (local_rect.min.y * scale_y);

        // 我们在 Y 轴和 X 轴加上正负 5.0 Points 的容错边界（允许字符部分擦边也被收纳进来）
        let target_min_x = pdf_min_x - 5.0;
        let target_max_x = pdf_max_x + 5.0;
        let target_min_y = pdf_min_y - 5.0;
        let target_max_y = pdf_max_y + 5.0;

        let mut matched_chars = Vec::new();
        let chars_list = page_text.chars();
        let total_chars = chars_list.len();

        // 3. 遍历全页所有的字符，只保留在我们选区范围内的字符
        for i in 0..total_chars {
            if let Ok(c) = chars_list.get(i) {
                if let Ok(char_rect) = c.loose_bounds() {
                    let char_x = char_rect.left().value;
                    let char_y = char_rect.bottom().value;

                    // 判定坐标是否在选区内
                    if char_x >= target_min_x 
                        && char_x <= target_max_x 
                        && char_y >= target_min_y 
                        && char_y <= target_max_y 
                    {
                        if let Some(ch) = c.unicode_char() {
                            matched_chars.push(ch.to_string());
                        }
                    }
                }
            }
        }

        // 4. 将匹配到的字符拼接起来并返回
        if !matched_chars.is_empty() {
            Some(matched_chars.concat())
        } else {
            None
        }
    }
}
