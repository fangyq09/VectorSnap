use pdfium_render::prelude::{PdfColor, PdfPoints, PdfPagePathObject,PdfPageObjectsCommon};
use image::{GenericImageView, GenericImage};

use crate::PdfApp;


#[derive(Clone)]
pub struct EraseStroke {
    pub points: Vec<egui::Pos2>,
    pub brush_size: f32,
		pub color: egui::Color32,
}
impl Default for EraseStroke {
	fn default() -> Self {
		Self {
			points: Vec::new(),
			brush_size: 20.0,
			color: egui::Color32::WHITE,
		}
	}
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolMode {
    Pen,
    Eraser,
}
#[derive(Clone, Copy, Debug)]
pub struct StrokePoint {
    pub pos: egui::Pos2, // 归一化坐标 (0.0 - 1.0)
    pub width: f32,      // 该点的最终物理宽度
}
#[derive(Clone)]
pub struct Stroke {
    pub points: Vec<StrokePoint>, // 这一条线包含的所有点
    pub color: egui::Color32,     // 这一条线专属的颜色
}
pub enum ColorShape {
    Circle,
    Rect,
}
#[derive(Clone)]
pub enum Annotation {
    Pen(Stroke),
    Eraser(EraseStroke),
}

//笔迹
impl PdfApp {
    // 渲染笔迹：带压力和颜色
    pub fn draw_pen_on_image(&self, img: &mut image::DynamicImage, stroke: &Stroke, w: u32, h: u32) {
        if stroke.points.len() < 2 { return; }
        
        let rgba = stroke.color.to_array(); // [r, g, b, a]
        let color = image::Rgba([rgba[0], rgba[1], rgba[2], rgba[3]]);

        // 复刻贝塞尔逻辑
        if stroke.points.len() < 3 {
            let p1 = stroke.points[0];
            let p2 = stroke.points[1];
            self.draw_line_on_image(img, p1.pos, p2.pos, p2.width, color, w, h);
        } else {
            for i in 0..stroke.points.len() - 2 {
                let p0 = stroke.points[i];
                let p1 = stroke.points[i+1];
                let p2 = stroke.points[i+2];

                let mid_start = p0.pos.lerp(p1.pos, 0.5);
                let mid_end = p1.pos.lerp(p2.pos, 0.5);

                let steps = 5;
                for s in 0..steps {
                    let t = s as f32 / steps as f32;
                    let t_next = (s + 1) as f32 / steps as f32;

                    let pos_curr = mid_start.lerp(p1.pos, t).lerp(p1.pos.lerp(mid_end, t), t);
                    let pos_next = mid_start.lerp(p1.pos, t_next).lerp(p1.pos.lerp(mid_end, t_next), t_next);
                    let current_width = p1.width + (p2.width - p1.width) * t;

                    self.draw_line_on_image(img, pos_curr, pos_next, current_width, color, w, h);
                }
            }
        }
    }

    // 渲染橡皮擦
   pub fn draw_eraser_on_image(&self, img: &mut image::DynamicImage, stroke: &EraseStroke, w: u32, h: u32) {
        let rgba = stroke.color.to_array();
        let color = image::Rgba([rgba[0], rgba[1], rgba[2], rgba[3]]);
        
        for window in stroke.points.windows(2) {
            self.draw_line_on_image(img, window[0], window[1], stroke.brush_size, color, w, h);
        }
    }

    // 底层绘制方法：将归一化坐标转换为像素坐标并绘制
   pub fn draw_line_on_image(&self, img: &mut image::DynamicImage, p1: egui::Pos2, p2: egui::Pos2, width: f32, color: image::Rgba<u8>, w: u32, h: u32) {
        let x1 = (p1.x * w as f32) as i32;
        let y1 = (p1.y * h as f32) as i32; // 注意：如果是 top-left 坐标系，这里可能不需要 1.0 - p.y，取决于你 to_normalized 的实现
        let x2 = (p2.x * w as f32) as i32;
        let y2 = (p2.y * h as f32) as i32;

        // 这里调用你原来的 draw_line，但增加颜色参数
        draw_line_with_color(img, x1, y1, x2, y2, (width / 2.0) as i32, color);
    }
}
fn draw_circle_with_color(img: &mut image::DynamicImage, cx: i32, cy: i32, radius: i32, color: image::Rgba<u8>) {
    let (w, h) = img.dimensions();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            if dx * dx + dy * dy <= radius * radius {
                let x = cx + dx;
                let y = cy + dy;
                if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                    img.put_pixel(x as u32, y as u32, color);
                }
            }
        }
    }
}
fn draw_line_with_color(
    img: &mut image::DynamicImage,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    thickness: i32,
    color: image::Rgba<u8>,
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };

    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;

    loop {
        // 在当前点画一个圆来表示线条的粗细
        // 注意：thickness 实际上是半径
        draw_circle_with_color(img, x, y, thickness, color);

        if x == x1 && y == y1 {
            break;
        }

        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

//保存修改后的pdf
impl PdfApp {
	pub fn save_as_pdf(&mut self, output_path: std::path::PathBuf) -> Result<(), Box<dyn std::error::Error>> {
		let doc = self.pdf_doc.as_ref().ok_or("No document loaded")?;

		for (&page_index, annos) in &self.annotations {
			let mut page = doc.pages().get(page_index as u16)?;

			// 预先获取页面度量信息
			let Some(metrics) = self.get_page_metrics_tuple(&page) else { continue; };

			// 核心：按用户操作的先后顺序写入 PDF
			for anno in annos {
				match anno {
					Annotation::Pen(stroke) => {
						self.apply_single_pen_stroke(doc, &mut page, stroke, metrics)?;
					}
					Annotation::Eraser(stroke) => {
						self.apply_single_eraser(doc, &mut page, stroke, metrics)?;
					}
				}
			}
		}

		doc.save_to_file(&output_path)?;
		Ok(())
	}

	// 辅助函数，将 metrics 转为元组方便传递
	pub fn get_page_metrics_tuple(&self, page: &pdfium_render::prelude::PdfPage) -> Option<(f32, f32, f32, f32, f32)> {
		let bounds = page.boundaries().crop().ok().or_else(|| page.boundaries().media().ok())?.bounds;
		let left = bounds.left().value;
		let bottom = bounds.bottom().value;
		let pw = (bounds.right().value - left).abs();
		let ph = (bounds.top().value - bottom).abs();
		let scale_factor = pw / self.last_pdf_width.max(1.0);
		Some((left, bottom, pw, ph, scale_factor))
	}
}
impl PdfApp {
	// 处理单条擦除
	pub fn apply_single_eraser<'a>(
		&self,
		doc: &pdfium_render::prelude::PdfDocument<'a>,
		page: &mut pdfium_render::prelude::PdfPage<'a>,
		stroke: &EraseStroke,
		metrics: (f32, f32, f32, f32, f32),
	) -> Result<(), Box<dyn std::error::Error>> {
		let (left, bottom, pw, ph, scale_factor) = metrics;

		let stroke_width = PdfPoints::new(stroke.brush_size * scale_factor);
		let stroke_color = PdfColor::new(stroke.color.r(), stroke.color.g(), stroke.color.b(), stroke.color.a());

		let map = |p: &egui::Pos2| {
			let x = left + p.x * pw;
			let y = bottom + (1.0 - p.y) * ph;
			(PdfPoints::new(x), PdfPoints::new(y))
		};

		let (sx, sy) = map(&stroke.points[0]);
		let mut path = PdfPagePathObject::new(doc, sx, sy, Some(stroke_color), Some(stroke_width), None)?;

		for p in stroke.points.iter().skip(1) {
			let (nx, ny) = map(p);
			let _ = path.line_to(nx, ny);
		}
		page.objects_mut().add_path_object(path)?;
		Ok(())
	}

	// 处理单条笔迹 (含压力感应插值)
	pub fn apply_single_pen_stroke<'a>(
		&self,
		doc: &pdfium_render::prelude::PdfDocument<'a>,
		page: &mut pdfium_render::prelude::PdfPage<'a>,
		stroke: &Stroke,
		metrics: (f32, f32, f32, f32, f32),
	) -> Result<(), Box<dyn std::error::Error>> {
		let (left, bottom, pw, ph, scale_factor) = metrics;

		let map_pos = |p: egui::Pos2| {
			let x = left + p.x * pw;
			let y = bottom + (1.0 - p.y) * ph;
			(PdfPoints::new(x), PdfPoints::new(y))
		};

		let stroke_color = Some(PdfColor::new(stroke.color.r(), stroke.color.g(), stroke.color.b(), stroke.color.a()));

		if stroke.points.len() < 3 {
			let (sx, sy) = map_pos(stroke.points[0].pos);
			let (ex, ey) = map_pos(stroke.points[1].pos);
			let width = Some(PdfPoints::new(stroke.points[1].width * scale_factor));
			let mut path = PdfPagePathObject::new(doc, sx, sy, stroke_color, width, None)?;
			path.line_to(ex, ey).ok();
			page.objects_mut().add_path_object(path)?;
		} else {
			for i in 0..stroke.points.len() - 2 {
				let p0 = &stroke.points[i];
				let p1 = &stroke.points[i+1];
				let p2 = &stroke.points[i+2];

				let mid_start = p0.pos.lerp(p1.pos, 0.5);
				let mid_end = p1.pos.lerp(p2.pos, 0.5);

				let steps = 5;
				for s in 0..steps {
					let t = s as f32 / steps as f32;
					let t_next = (s + 1) as f32 / steps as f32;

					let pos_curr = mid_start.lerp(p1.pos, t).lerp(p1.pos.lerp(mid_end, t), t);
					let pos_next = mid_start.lerp(p1.pos, t_next).lerp(p1.pos.lerp(mid_end, t_next), t_next);
					let current_width = (p1.width + (p2.width - p1.width) * t) * scale_factor;

					let (sx, sy) = map_pos(pos_curr);
					let (nx, ny) = map_pos(pos_next);

					let mut path_seg = PdfPagePathObject::new(doc, sx, sy, stroke_color, Some(PdfPoints::new(current_width)), None)?;
					path_seg.line_to(nx, ny).ok();
					page.objects_mut().add_path_object(path_seg)?;
				}
			}
		}
		Ok(())
	}
}

