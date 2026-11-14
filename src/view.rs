use eframe::egui;
use pdfium_render::prelude::{
    PdfPageAnnotationCommon, PdfPageObjectType, PdfPageObjectsCommon, PdfPageRenderRotation,
    PdfRenderConfig,
};

use crate::{CachedLink, PdfApp};

impl PdfApp {
	pub(crate) fn render_pdf_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

//impl PdfApp {
//	fn paint_pdf_surface(
//		&mut self,
//		ui: &mut egui::Ui,
//		ctx: &egui::Context,
//		page: &pdfium_render::prelude::PdfPage,
//		display_size: egui::Vec2,
//		native_size: (f32, f32), // (width, height)
//		angle: f32,
//		total_pages: usize,
//	) {
//		let (native_width, native_height) = native_size;
//
//		let scroll_source = if self.is_edit_mode {
//			egui::scroll_area::ScrollSource {
//				scroll_bar: true,
//				drag: false, 
//				mouse_wheel: true,
//			}
//		} else {
//			egui::scroll_area::ScrollSource::ALL
//		};
//
//		let outer_viewport = ui.available_size_before_wrap();
//
//		egui::ScrollArea::both()
//			.auto_shrink([false; 2])
//			.scroll_source(scroll_source)
//			.show(ui, |ui| {
//				if self.scroll_delta != egui::Vec2::ZERO {
//					ui.scroll_with_delta(self.scroll_delta);
//					self.scroll_delta = egui::Vec2::ZERO;
//				}
//
//				//let available_width = ui.available_width();
//				//let is_overflowing = display_size.x > available_width;
//
//				//// 1. 偏移与缩放检测逻辑 
//				//let x_offset = if !is_overflowing { (available_width - display_size.x) / 2.0 } else { 0.0 };
//
//				//if is_overflowing && self.last_pdf_width <= available_width 
//				//	&& self.last_pdf_width != display_size.x {
//				//		ui.scroll_with_delta(egui::vec2(-100.0, 0.0));
//				//}
//
//				//let content_size = egui::vec2(available_width.max(display_size.x), display_size.y);
//				//let (outer_rect, _) = ui.allocate_at_least(content_size, egui::Sense::hover());
//
//				//let pdf_rect = egui::Rect::from_min_size(
//				//	egui::pos2(outer_rect.min.x + x_offset, outer_rect.min.y),
//				//	display_size
//				//);
//                if self.scroll_delta != egui::Vec2::ZERO {
//                    ui.scroll_with_delta(self.scroll_delta);
//                    self.scroll_delta = egui::Vec2::ZERO;
//                }
//
//                // 真实可视区域（clip_rect 已扣掉滚动条占位），双保险防止拿到无限大
//                let viewport = outer_viewport.min(ui.clip_rect().size());
//
//                // 内容尺寸：小于视口时撑满视口（才能居中），大于视口时等于页面（才能滚动）
//                let content_size = viewport.max(display_size);
//                if let Some((p, a, prev)) = self.last_layout {
//                    if p == self.current_page && (a - angle).abs() < 0.1 {
//                        let grow = display_size - prev;
//                        let mut adjust = egui::Vec2::ZERO;
//                        if display_size.x > viewport.x { adjust.x = grow.x * 0.5; }
//                        if display_size.y > viewport.y { adjust.y = grow.y * 0.5; }
//                        if adjust.length_sq() > 0.01 { ui.scroll_with_delta(adjust); }
//                    }
//                }
//                self.last_layout = Some((self.current_page, angle, display_size));
//                let (outer_rect, _) = ui.allocate_at_least(content_size, egui::Sense::hover());
//
//                // ★ 横竖都居中；超出视口的轴 offset 自动为 0，交给滚动条
//                let offset = (content_size - display_size) * 0.5;
//                let pdf_rect = egui::Rect::from_min_size(outer_rect.min + offset, display_size);
//
//                // ↓↓↓ 下面像素对齐、painter.image、标注、交互全部保持原样 ↓↓↓
//                let ppp = ui.ctx().pixels_per_point();
//                let aligned_rect = egui::Rect::from_min_size(
//                    egui::pos2((pdf_rect.min.x * ppp).round() / ppp,
//                               (pdf_rect.min.y * ppp).round() / ppp),
//                    display_size,
//                );
//
//				// 2. 像素对齐
//				let ppp = ui.ctx().pixels_per_point();
//				let aligned_rect = egui::Rect::from_min_size(
//					egui::pos2((pdf_rect.min.x * ppp).round() / ppp, (pdf_rect.min.y * ppp).round() / ppp),
//					display_size
//				);
//
//				self.last_pdf_width = aligned_rect.width();
//				self.last_pdf_height = aligned_rect.height();
//
//				let painter = ui.painter_at(aligned_rect);
//				if let Some(tex) = &self.texture {
//					painter.image(tex.id(), aligned_rect,
//					egui::Rect::from_min_max(egui::pos2(0.0, 0.0),
//					egui::pos2(1.0, 1.0)), 
//					egui::Color32::WHITE);
//				}
//
//				self.paint_static_annotations(ui, aligned_rect);
//
//				if self.is_edit_mode {
//					self.handle_edit_mode(ui, aligned_rect);
//				} else {
//
//					// 3. 交互响应
//					//let mut response = ui.interact(aligned_rect, 
//					//	ui.id().with("pdf_surf"), 
//					//	egui::Sense::click_and_drag());
//					//response.rect = aligned_rect;
//					let response = ui.interact(
//						aligned_rect, 
//						ui.id().with("pdf_surf"), 
//						egui::Sense::click_and_drag()
//					);
//
//					// --- 业务集成 ---
//					if response.clicked() && ui.input(|i| i.modifiers.ctrl) {
//						if let Some(pos) = response.interact_pointer_pos() { self.sync_to_latex(pos, aligned_rect); }
//					}
//
//					// --- 更新链接缓存 ---
//					let page_changed = self.last_link_page != Some(self.current_page);
//					let rect_changed = if let Some(last) = self.last_link_rect {
//						(last.min.x - aligned_rect.min.x).abs() > 0.1 || (last.min.y - aligned_rect.min.y).abs() > 0.1
//					} else { true };
//
//					if page_changed || rect_changed {
//						self.update_link_cache(&page, aligned_rect, native_width, native_height);
//						self.last_link_page = Some(self.current_page);
//						self.last_link_rect = Some(aligned_rect);
//					}
//
//
//					if angle == 0.0 && self.handle_link_interaction(ctx, &response, total_pages) {
//						ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
//					}
//					self.handle_selection_interaction(&response, &painter, ctx, page);
//				}
//			});
//	}
//}

//PDF页面绘制（居中布局）
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

		// ★A 视口尺寸（必须在 ScrollArea 外面取）
		let outer_viewport = ui.available_size_before_wrap();
		self.last_viewport = outer_viewport;

		// ★B 同页同角度但尺寸变了 => 缩放/窗口变化，需要重新居中
		let size_changed = match self.last_layout {
			Some((p, a, size)) => {
				p == self.current_page
					&& (a - angle).abs() < 0.1
					&& (size - display_size).length_sq() > 0.25
			}
			None => true,
		};

		// ★C 先构建 ScrollArea，尺寸变化时显式设置居中偏移
		let mut scroll_area = egui::ScrollArea::both()
			.auto_shrink([false; 2])
			.scroll_source(scroll_source);

		if size_changed {
			let centered = ((display_size - outer_viewport) * 0.5).max(egui::Vec2::ZERO);
			scroll_area = scroll_area.scroll_offset(centered);
		}

		scroll_area.show(ui, |ui| {
			if self.scroll_delta != egui::Vec2::ZERO {
				ui.scroll_with_delta(self.scroll_delta);
				self.scroll_delta = egui::Vec2::ZERO;
			}

			// ★D 新的居中布局计算（取代原 available_width / x_offset / -100 hack）
			let viewport = outer_viewport.min(ui.clip_rect().size());
			let content_size = viewport.max(display_size);
			let (outer_rect, _) = ui.allocate_at_least(content_size, egui::Sense::hover());

			let offset = (content_size - display_size) * 0.5;
			let pdf_rect = egui::Rect::from_min_size(outer_rect.min + offset, display_size);

			// ===== 以下全部是你原来的代码，一行没改 =====
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

		// ★E 记录本帧尺寸（必须在 show() 闭包外面）
		self.last_layout = Some((self.current_page, angle, display_size));
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
