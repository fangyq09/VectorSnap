use eframe::egui;
use lopdf::{Document, Object};
use lopdf::content::Operation;
use std::path::Path;

use crate::PdfApp;

//合并PDF
impl PdfApp {
	pub fn render_merge_window(&mut self, ctx: &egui::Context) {
		if !self.show_merge_window { return; }

		// 1. 定义局部变量用于同步右上角关闭按钮
		let mut open = self.show_merge_window;

		let window_response = egui::Window::new("Merge Files to PDF")
			.open(&mut open)
			.resizable(true)
			.default_width(400.0)
			.show(ctx, |ui| {
				ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
				ui.vertical(|ui| {
					ui.horizontal(|ui| {
						ui.checkbox(
							&mut self.force_a4,
							"Force all pages to A4"
						)
							.on_hover_text(
								"Resize all pages to A4 paper size.\n\
				 				Content is proportionally scaled and centered.\n\
				 				Original page orientation is preserved."
							);
					});

					ui.separator();

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
						let mut move_to_first = None;
						let mut move_to_last = None;
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


										// 移动到末尾
										ui.add_enabled_ui(i < len - 1, |ui| {
											if ui.button("⏭").on_hover_text("Move to Last").clicked() {
												move_to_last = Some(i);
											}
										});

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

										// 移动到首行
										ui.add_enabled_ui(i > 0, |ui| {
											if ui.button("⏮").on_hover_text("Move to First").clicked() {
												move_to_first = Some(i);
											}
										});
                });
							});
						}
						//if let Some(idx) = to_remove { self.merge_file_list.remove(idx); }
						//if let Some(idx) = move_up { self.merge_file_list.swap(idx, idx - 1); }
						//if let Some(idx) = move_down { self.merge_file_list.swap(idx, idx + 1); }
						if let Some(idx) = to_remove {
							self.merge_file_list.remove(idx);
						} else if let Some(idx) = move_to_first {
							let item = self.merge_file_list.remove(idx);
							self.merge_file_list.insert(0, item);
						} else if let Some(idx) = move_to_last {
							let item = self.merge_file_list.remove(idx);
							self.merge_file_list.push(item);
						} else if let Some(idx) = move_up {
							self.merge_file_list.swap(idx, idx - 1);
						} else if let Some(idx) = move_down {
							self.merge_file_list.swap(idx, idx + 1);
						}
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

			// 处理拖入合并窗口的文件
			if let Some(response) = window_response {
				self.handle_merge_window_drag_drop(
					ctx,
					response.response.rect,
				);
			}

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
							}
						}
					}
				}
			}//endfor

			if has_content {
				println!("合并PDF总页数: {}", new_doc.pages().len());

				match new_doc.save_to_file(&save_path) {
					Ok(_) => {
						// 如果勾选 A4，则进一步调整所有页面
						if self.force_a4 {
							match force_all_pages_to_a4(&save_path) {
								Ok(_) => {
									println!("所有页面已成功调整为 A4: {:?}", save_path);
								}
								Err(e) => {
									eprintln!("A4 页面调整失败: {:?}", e);
								}
							}
						}

						println!("保存成功: {:?}", save_path);
					}

					Err(e) => {
						eprintln!("保存失败: {:?}", e);
					}
				}
			}

		}
	}
}

// 将所有 PDF 页面适配为 A4，保留原始方向和内容比例
fn force_all_pages_to_a4(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut doc = Document::load(path)?;

    // A4 尺寸，单位：PDF points
    const A4_W: f32 = 595.276;
    const A4_H: f32 = 841.890;

    let pages = doc.get_pages();

    for (_page_number, page_id) in pages {
        // 获取页面 MediaBox（考虑从父节点继承）
        let media_box = {
            let mut current_id = page_id;
            let mut result = None;

            loop {
                let dict = doc.get_dictionary(current_id)?;

                if let Ok(value) = dict.get(b"MediaBox") {
                    result = match value {
                        Object::Array(arr) => Some(arr.clone()),
                        Object::Reference(id) => {
                            match doc.get_object(*id)? {
                                Object::Array(arr) => Some(arr.clone()),
                                _ => None,
                            }
                        }
                        _ => None,
                    };

                    if result.is_some() {
                        break;
                    }
                }

                match dict.get(b"Parent") {
                    Ok(Object::Reference(parent_id)) => {
                        current_id = *parent_id;
                    }
                    _ => break,
                }
            }

            result.ok_or("Cannot find page MediaBox")?
        };

        if media_box.len() != 4 {
            return Err("Invalid MediaBox".into());
        }

        let get_num = |obj: &Object| -> Result<f32, Box<dyn std::error::Error>> {
            match obj {
                Object::Integer(v) => Ok(*v as f32),
                Object::Real(v) => Ok(*v),
                _ => Err("Invalid MediaBox coordinate".into()),
            }
        };

        let x0 = get_num(&media_box[0])?;
        let y0 = get_num(&media_box[1])?;
        let x1 = get_num(&media_box[2])?;
        let y1 = get_num(&media_box[3])?;

        let old_w = x1 - x0;
        let old_h = y1 - y0;

        if old_w <= 0.0 || old_h <= 0.0 {
            continue;
        }

        // 保留原页面方向：横向页面使用横向 A4
        let (target_w, target_h) = if old_w > old_h {
            (A4_H, A4_W)
        } else {
            (A4_W, A4_H)
        };

        // 等比例缩放，确保完整显示
        let scale = (target_w / old_w).min(target_h / old_h);

        // 居中
        let translate_x = (target_w - old_w * scale) / 2.0 - x0 * scale;
        let translate_y = (target_h - old_h * scale) / 2.0 - y0 * scale;

        // 获取原始页面内容
        let mut content = doc.get_and_decode_page_content(page_id)?;

        // q：保存原始图形状态
        content.operations.insert(
            0,
            Operation::new("q", vec![]),
        );

        // cm：应用缩放和平移矩阵
        content.operations.insert(
            1,
            Operation::new(
                "cm",
                vec![
                    Object::Real(scale),
                    Object::Integer(0),
                    Object::Integer(0),
                    Object::Real(scale),
                    Object::Real(translate_x),
                    Object::Real(translate_y),
                ],
            ),
        );

        // Q：恢复图形状态
        content.operations.push(
            Operation::new("Q", vec![]),
        );

        // 写回页面内容
        doc.change_page_content(page_id, content.encode()?)?;

        // 修改页面尺寸
        let page_dict = doc.get_object_mut(page_id)?.as_dict_mut()?;

        let a4_box = Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Real(target_w),
            Object::Real(target_h),
        ]);

        page_dict.set("MediaBox", a4_box.clone());
        page_dict.set("CropBox", a4_box);

        // 避免旧的页面边界继续限制显示
        page_dict.remove(b"TrimBox");
        page_dict.remove(b"BleedBox");
        page_dict.remove(b"ArtBox");
    }

    // 保存调整后的 PDF
    doc.save(path)?;

    Ok(())
}

impl PdfApp {
    fn handle_merge_window_drag_drop(
        &mut self,
        ctx: &egui::Context,
        window_rect: egui::Rect,
    ) {
        // 获取拖入文件和鼠标位置
        let (dropped_paths, pointer_pos) = ctx.input(|i| {
            let paths = i.raw.dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect::<Vec<_>>();

            (paths, i.pointer.latest_pos())
        });

        // 只接收释放在合并窗口内部的文件
        if let Some(pos) = pointer_pos {
            if window_rect.contains(pos) {
                for path in dropped_paths {
                    let ext = path.extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();

                    // 支持 PDF 和常见图片
                    if ["pdf", "png", "jpg", "jpeg"].contains(&ext.as_str()) {
                        // 避免重复添加
                        if !self.merge_file_list.contains(&path) {
                            self.merge_file_list.push(path);
                        }
                    }
                }
            }
        }
    }
}
