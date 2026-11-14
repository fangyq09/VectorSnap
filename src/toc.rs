use eframe::egui;

use crate::{PdfApp, TocNode};

//目录（Bookmarks / Outline）
impl PdfApp {
	// 加载 PDF 后调用一次，把书签树缓存成 owned 数据
	pub(crate) fn build_toc(&mut self) {
		self.toc_expanded.clear();
		let nodes = {
			let Some(doc) = self.pdf_doc.as_ref() else { self.toc.clear(); return; };
			let mut counter = 0usize;
			doc.bookmarks().root()
				.map(|root| Self::collect_bookmarks(Some(root), &mut counter))
				.unwrap_or_default()
		};
		// 默认展开第一层
		for n in &nodes { self.toc_expanded.insert(n.id); }
		self.toc = nodes;
	}

	fn collect_bookmarks(
		first: Option<pdfium_render::prelude::PdfBookmark<'_>>,
		counter: &mut usize,
	) -> Vec<TocNode> {
		let mut out = Vec::new();
		let mut cur = first;
		while let Some(bm) = cur {
			let id = *counter;
			*counter += 1;
			let title = bm.title().unwrap_or_default();
			let page = bm.destination()
				.and_then(|d| d.page_index().ok())
				.map(|i| i as usize);
			let children = Self::collect_bookmarks(bm.first_child(), counter);
			out.push(TocNode {
				id,
				title: if title.trim().is_empty() { "(无标题)".to_string() } else { title },
				page,
				children,
			});
			cur = bm.next_sibling();
		}
		out
	}

	pub(crate) fn render_toc_panel(&mut self, ctx: &egui::Context) {
		if !self.show_toc || self.pdf_doc.is_none() { return; }
		let mut jump_to: Option<usize> = None;
		let mut inner_rect = egui::Rect::NOTHING;                 

		let panel_resp = egui::SidePanel::left("toc_panel")
			.exact_width(200.0)
			.show(ctx, |ui| {
				inner_rect = ui.max_rect();                        // 记录面板区域
				ui.add_space(6.0);
				ui.horizontal(|ui| {
					ui.strong("📑Table of Contents");
					ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
						if ui.button("✖").clicked() { self.show_toc = false; }
					});
				});
				ui.separator();

				if self.toc.is_empty() {
					ui.add_space(10.0);
					ui.weak("This PDF has no built-in table of contents");
					return;
				}

				egui::ScrollArea::both() // 同时开启水平和垂直滚动
					.auto_shrink([false; 2])
					.show(ui, |ui| {
						ui.allocate_ui_with_layout(
							egui::vec2(ui.available_width().max(200.0), 0.0),
							egui::Layout::top_down(egui::Align::LEFT),
							|ui| {
								Self::render_toc_nodes(
									ui,
									&self.toc,
									&mut self.toc_expanded,
									self.current_page,
									&mut jump_to,
								);
							},
						);
					});
			});

		// ★ 面板真实区域（并集：兼容 frame 内边距导致 max_rect 略小的情况）
		let panel_rect = inner_rect.union(panel_resp.response.rect);

		// ★ 点击面板之外 => 自动关闭
		let just_opened = ctx.input(|i| i.time) - self.toc_opened_at < 0.2;
		if !just_opened {
			let clicked_outside = ctx.input(|i| {
				i.pointer.any_pressed()
					&& i.pointer.interact_pos()
						.or_else(|| i.pointer.hover_pos())
						.map_or(false, |p| !panel_rect.contains(p))
			});
			if clicked_outside {
				self.show_toc = false;
				ctx.request_repaint();
			}
		}

		if let Some(p) = jump_to {
			if let Some(doc) = &self.pdf_doc {
				let total = doc.pages().len() as usize;
				self.current_page = p.min(total.saturating_sub(1));
				self.texture = None;
				ctx.request_repaint();
			}
		}
	}

	fn render_toc_nodes(
		ui: &mut egui::Ui,
		nodes: &[TocNode],
		expanded: &mut std::collections::HashSet<usize>,
		current_page: usize,
		jump_to: &mut Option<usize>,
	) {
		for node in nodes {
			let has_children = !node.children.is_empty();
			let is_open = expanded.contains(&node.id);

			ui.horizontal(|ui| {
				if has_children {
					let arrow = if is_open { "▾" } else { "▸" };
					if ui.add(egui::Button::new(arrow).frame(false)).clicked() {
						if is_open { expanded.remove(&node.id); } else { expanded.insert(node.id); }
					}
				} else {
					ui.add_space(16.0);
				}

				let selected = node.page == Some(current_page);
				let resp = ui.selectable_label(selected, &node.title)
					.on_hover_text(&node.title);
				if resp.clicked() {
					if let Some(p) = node.page { *jump_to = Some(p); }
				}
			});

			if has_children && is_open {
				ui.indent("toc_indent", |ui| {
					Self::render_toc_nodes(ui, &node.children, expanded, current_page, jump_to);
				});
			}
		}
	}
}
