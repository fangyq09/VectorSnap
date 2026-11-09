use eframe::egui;
use crate::PdfApp;
use std::collections::{HashSet};
use image::GenericImageView;
use pdfium_render::prelude:: PdfRenderConfig;

use crate::handwriting::{
    Annotation,
};

//提取页面
impl PdfApp {
	pub fn render_export_window(&mut self, ctx: &egui::Context) {
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

