use eframe::egui;

use crate::PdfApp;

//合并PDF
impl PdfApp {
	pub fn render_merge_window(&mut self, ctx: &egui::Context) {
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
