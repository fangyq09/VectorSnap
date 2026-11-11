use std::path::{Path, PathBuf};
use std::process::{Stdio, Command};
//use std::hash::{Hash, Hasher};
//use std::collections::hash_map::DefaultHasher;
use std::thread;

#[derive(Clone)]
struct SynctexEditorConfig {
    editor_command: String,
    editor: Option<String>,
    vim_servername: Option<String>,
    nvim_path: String,
    pdf_path: PathBuf,
		synctex_dir: String,
}

use crate::PdfApp;
use crate::spawn_and_reap;
use crate::ipc::pdf_ipc_hash;

//fn spawn_and_reap(child: std::io::Result<Child>) {
//    if let Ok(mut child) = child {
//        let _ = thread::spawn(move || {
//            let _ = child.wait();
//        });
//    }
//}
//Synctex反向搜索
//impl PdfApp {
//	pub fn sync_to_latex(&self, click_pos: egui::Pos2, ui_rect: egui::Rect) {
//		let path = match &self.pdf_path {
//			Some(p) => p,
//			None => return,
//		};
//
//		let page_index = self.current_page;
//		let Some(doc) = &self.pdf_doc else { return };
//		let Ok(page) = doc.pages().get(page_index as u16) else { return };
//
//		// 1. 计算相对比例 (0.0 - 1.0)
//		let local_x = (click_pos.x - ui_rect.min.x) / ui_rect.width();
//		let local_y = (click_pos.y - ui_rect.min.y) / ui_rect.height();
//
//		// 2. 获取 PDF 页面原始尺寸 (Points, 72 DPI)
//		// 考虑旋转逻辑，与你 perform_high_res_crop 保持一致
//		let angle = self.rotations.get(page_index).copied().unwrap_or(0.0);
//		let is_sideways = ((angle / 90.0).round() as i32).abs() % 2 != 0;
//
//		let (p_w, p_h) = if is_sideways {
//			(page.height().value, page.width().value)
//		} else {
//			(page.width().value, page.height().value)
//		};
//
//		// 3. 映射到 PDF 坐标系 (注意：SyncTex 的 y 轴通常是从顶端向下的点数)
//		let target_x = local_x * p_w;
//		let target_y = local_y * p_h;
//
//		// 4. 调用 synctex 命令行工具
//		// 这里的命令：synctex edit -o "页码:x:y:pdf文件名"
//		//let output = Command::new("synctex")
//		//	.arg("edit")
//		//	.arg("-o")
//		//	.arg(format!("{}:{}:{}:{}", page_index + 1, target_x, target_y, path.display()))
//		//	.output();
//		let mut cmd = Command::new("synctex");
//
//		cmd.arg("edit")
//			.arg("-o")
//			.arg(format!("{}:{}:{}:{}", page_index + 1, target_x, target_y, path.display()));
//
//		// 如果配置了 synctex_dir，则指定 SyncTeX 文件目录
//		let synctex_dir = self.config.synctex_dir.trim();
//
//		if !synctex_dir.is_empty() {
//			let dir = PathBuf::from(synctex_dir);
//
//			// 相对路径以 PDF 所在目录为基准
//			let resolved_dir = if dir.is_absolute() {
//				dir
//			} else {
//				path.parent()
//					.unwrap_or(std::path::Path::new("."))
//					.join(dir)
//			};
//
//			if resolved_dir.is_dir() {
//				cmd.arg("-d").arg(resolved_dir);
//			} 
//		}
//
//		let output = cmd.output();
//
//		if let Ok(out) = output {
//			let result = String::from_utf8_lossy(&out.stdout);
//			//println!("SyncTex Result: {}", result);
//
//			// 5. 解析结果并调用编辑器 (以 VS Code 为例)
//			// SyncTex 返回格式通常包含: 'Input:main.tex', 'Line:123', 'Column:1'
//			self.open_editor_from_synctex(&result);
//		}
//	}
//}
impl PdfApp {
	pub fn sync_to_latex(&self, click_pos: egui::Pos2, ui_rect: egui::Rect) {
		let path = match &self.pdf_path {
			Some(p) => p,
			None => return,
		};

		let editor_config = SynctexEditorConfig {
			editor_command: self.config.editor_command.clone(),
			editor: self.editor.clone(),
			vim_servername: self.vim_servername.clone(),
			nvim_path: self.config.nvim_path.clone(),
			pdf_path: path.to_path_buf(),
			synctex_dir: self.config.synctex_dir.trim().to_string(),
		};

		let page_index = self.current_page;
		let Some(doc) = &self.pdf_doc else { return };
		let Ok(page) = doc.pages().get(page_index as u16) else { return };

		// 1. 计算相对比例 (0.0 - 1.0)
		let local_x = (click_pos.x - ui_rect.min.x) / ui_rect.width();
		let local_y = (click_pos.y - ui_rect.min.y) / ui_rect.height();

		// 2. 获取 PDF 页面原始尺寸 (Points, 72 DPI)
		// 考虑旋转逻辑，与你 perform_high_res_crop 保持一致
		let angle = self.rotations.get(page_index).copied().unwrap_or(0.0);
		let is_sideways = ((angle / 90.0).round() as i32).abs() % 2 != 0;

		let (p_w, p_h) = if is_sideways {
			(page.height().value, page.width().value)
		} else {
			(page.width().value, page.height().value)
		};

		// 3. 映射到 PDF 坐标系 (注意：SyncTex 的 y 轴通常是从顶端向下的点数)
		let target_x = local_x * p_w;
		let target_y = local_y * p_h;

		// 4. 调用 synctex 命令行工具
		// 这里的命令：synctex edit -o "页码:x:y:pdf文件名"
		//let output = Command::new("synctex")
		//	.arg("edit")
		//	.arg("-o")
		//	.arg(format!("{}:{}:{}:{}", page_index + 1, target_x, target_y, path.display()))
		//	.output();
		let mut cmd = Command::new("synctex");

		cmd.arg("edit")
			.arg("-o")
			.arg(format!("{}:{}:{}:{}", page_index + 1, target_x, target_y, path.display()));

		// 如果配置了 synctex_dir，则指定 SyncTeX 文件目录
		//let synctex_dir = self.config.synctex_dir.trim();
		let synctex_dir = editor_config.synctex_dir.trim();

		if !synctex_dir.is_empty() {
			let dir = PathBuf::from(synctex_dir);

			// 相对路径以 PDF 所在目录为基准
			let resolved_dir = if dir.is_absolute() {
				dir
			} else {
				path.parent()
					.unwrap_or(std::path::Path::new("."))
					.join(dir)
			};

			if resolved_dir.is_dir() {
				cmd.arg("-d").arg(resolved_dir);
			} 
		}

		//let output = cmd.output();

		//if let Ok(out) = output {
		//	let result = String::from_utf8_lossy(&out.stdout);
		//	//println!("SyncTex Result: {}", result);

		//	// 5. 解析结果并调用编辑器 (以 VS Code 为例)
		//	// SyncTex 返回格式通常包含: 'Input:main.tex', 'Line:123', 'Column:1'
		//	self.open_editor_from_synctex(&result);
		//}
		thread::spawn(move || {
			let output = cmd.output();
			let Ok(out) = output else { return; };
			if !out.status.success() { return; }
			let result = String::from_utf8_lossy(&out.stdout).into_owned();
			open_editor_from_synctex(&editor_config, &result);
		});
	}
}
//impl PdfApp {
//	fn open_editor_from_synctex(&self, synctex_output: &str) {
//		// 1. 解析 SyncTeX 输出
//		let mut file = "";
//		let mut line = "";
//
//		for l in synctex_output.lines() {
//			if let Some(v) = l.strip_prefix("Input:") {
//				file = v.trim();
//			}
//
//			if let Some(v) = l.strip_prefix("Line:") {
//				line = v.trim();
//			}
//		}
//
//		if file.is_empty() || line.is_empty() {
//			return;
//		}
//
//		// 2. 获取编辑器配置
//		//let raw_cmd = self.config.editor_command.trim();
//		let raw_cmd = self.editor
//			.as_deref()
//			.unwrap_or(self.config.editor_command.trim())
//			.trim();
//
//		if raw_cmd.is_empty() {
//			return;
//		}
//
//		// 提取可执行文件名称，支持完整路径
//		let base_editor = Path::new(raw_cmd)
//			.file_stem()
//			.and_then(|s| s.to_str())
//			.unwrap_or("")
//			.to_lowercase();
//
//		// 3. 根据编辑器类型分别执行
//		match base_editor.as_str() {
//
//			// ==========================================
//			// GVim / Vim
//			// ==========================================
//			"gvim" | "vim" => {
//				// 与 gvim-tex.sh 保持完全一致的 ServerName 算法
//				self.open_gvim_from_synctex(raw_cmd, file, line);
//			}
//
//			// ==========================================
//			// Neovim
//			// ==========================================
//			"nvim" | "neovim" | "neovim-qt" => {
//				self.open_nvim_from_synctex(raw_cmd, file, line);
//			}
//
//			// ==========================================
//			// Neovide
//			// ==========================================
//
//			"neovide" => {
//				self.open_neovide_from_synctex(raw_cmd, file, line);
//			}
//
//			// ==========================================
//			// 其他编辑器
//			// ==========================================
//			_ => {
//				self.open_custom_editor_from_synctex(raw_cmd, file, line);
//			}
//		}
//	}
//}
fn open_editor_from_synctex(
    config: &SynctexEditorConfig,
    synctex_output: &str,
) {
    let mut file = "";
    let mut line = "";

    for l in synctex_output.lines() {
        if let Some(v) = l.strip_prefix("Input:") {
            file = v.trim();
        }

        if let Some(v) = l.strip_prefix("Line:") {
            line = v.trim();
        }
    }

    if file.is_empty() || line.is_empty() {
        return;
    }

    let raw_cmd = config.editor
        .as_deref()
        .unwrap_or(config.editor_command.trim())
        .trim();

    if raw_cmd.is_empty() {
        return;
    }

    let base_editor = Path::new(raw_cmd)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    match base_editor.as_str() {
        "gvim" | "vim" => {
            open_gvim_from_synctex(config, raw_cmd, file, line);
        }

        "nvim" | "neovim" | "neovim-qt" => {
            open_nvim_from_synctex(config, raw_cmd, file, line);
        }

        "neovide" => {
            open_neovide_from_synctex(config, raw_cmd, file, line);
        }

        _ => {
            open_custom_editor_from_synctex(raw_cmd, file, line);
        }
    }
}
//open gvim
//impl PdfApp {
//	fn open_gvim_from_synctex(&self, raw_cmd: &str, file: &str, line: &str) {
//		let pdf_path = match &self.pdf_path {
//			Some(p) => p,
//			None => return,
//		};
//		//let server_name = self.vim_servername
//		//	.as_deref()
//		//	.filter(|s| !s.trim().is_empty())
//		//	.map(|s| s.to_string())
//		//	.unwrap_or_else(|| {
//		//		file.chars()
//		//			.map(|c| { if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' } })
//		//			.collect::<String>()
//		//	});
//
//		let server_name = self.vim_servername
//			.as_deref()
//			.filter(|s| !s.trim().is_empty())
//			.map(str::to_string)
//			.unwrap_or_else(|| {
//				pdf_path
//					.to_string_lossy()
//					.chars()
//					.map(|c| {
//						if c.is_ascii_alphanumeric() {
//							c.to_ascii_uppercase()
//						} else {
//							'_'
//						}
//					})
//				.collect::<String>()
//			});
//
//		let _ = Command::new(raw_cmd)
//			.arg("--servername")
//			.arg(&server_name)
//			.arg("--remote-silent")
//			.arg(format!("+{}", line))
//			.arg(file)
//			.spawn();
//		}
//}
	fn open_gvim_from_synctex(config: &SynctexEditorConfig, raw_cmd: &str, file: &str, line: &str) {
		let pdf_path = &config.pdf_path;
		let server_name = &config.vim_servername
			.as_deref()
			.filter(|s| !s.trim().is_empty())
			.map(str::to_string)
			.unwrap_or_else(|| {
				pdf_path
					.to_string_lossy()
					.chars()
					.map(|c| {
						if c.is_ascii_alphanumeric() {
							c.to_ascii_uppercase()
						} else {
							'_'
						}
					})
				.collect::<String>()
			});

		spawn_and_reap(
			Command::new(raw_cmd)
			.arg("--servername")
			.arg(&server_name)
			.arg("--remote-silent")
			.arg(format!("+{}", line))
			.arg(file)
			.spawn()
		);
	}
//open nvim
//impl PdfApp {
//	fn open_nvim_from_synctex(
//		&self,
//		raw_cmd: &str,
//		file: &str,
//		line: &str,
//	) {
//		let pdf_path = match &self.pdf_path {
//			Some(p) => p,
//			None => return,
//		};
//
//		// 尝试连接指定的 Neovim ServerName
//		let try_connect = |server: &str| -> bool {
//			Command::new(raw_cmd)
//				.arg("--server")
//				.arg(server)
//				.arg("--remote")
//				.arg(format!("+{}", line))
//				.arg(file)
//				.status()
//				.map(|status| status.success())
//				.unwrap_or(false)
//		};
//
//		// 1. 优先连接 Neovim 传来的 ServerName
//		if let Some(server_name) = self.vim_servername
//			.as_deref()
//				.filter(|s| !s.trim().is_empty())
//		{
//			if try_connect(server_name) {
//				return;
//			}
//		}
//
//		// 2. 使用 PDF 完整路径生成 Hash
//		//let normalized_path = pdf_path
//		//	.canonicalize()
//		//	.unwrap_or_else(|_| pdf_path.to_path_buf());
//
//		//let mut pdf_hash: u64 = 0xcbf29ce484222325;
//
//		//for byte in normalized_path.to_string_lossy().bytes() {
//		//	pdf_hash ^= byte as u64;
//		//	pdf_hash = pdf_hash.wrapping_mul(0x100000001b3);
//		//}
//
//		let pdf_hash = pdf_ipc_hash(pdf_path);
//		let socket_path = format!(
//			"/tmp/vectorsnap-nvim-{:016x}.sock",
//			pdf_hash
//		);
//
//		// 3. 尝试连接 PDF Hash 对应的 Neovim
//		if Path::new(&socket_path).exists() {
//			if try_connect(&socket_path) {
//				return;
//			}
//
//			// 连接失败，清理失效 socket
//			let _ = std::fs::remove_file(&socket_path);
//		}
//
//		// 4. 启动新的 Neovim 实例
//		spawn_and_reap(
//			Command::new(raw_cmd)
//			.arg("--listen")
//			.arg(&socket_path)
//			.arg(format!("+{}", line))
//			.arg(file)
//			.stdin(Stdio::null())
//			.stdout(Stdio::null())
//			.stderr(Stdio::null())
//			.spawn()
//			);
//	}
//}
	fn open_nvim_from_synctex(
		config: &SynctexEditorConfig,
		raw_cmd: &str,
		file: &str,
		line: &str,
	) {

		let pdf_path = &config.pdf_path;
		// 尝试连接指定的 Neovim ServerName
		let try_connect = |server: &str| -> bool {
			Command::new(raw_cmd)
				.arg("--server")
				.arg(server)
				.arg("--remote")
				.arg(format!("+{}", line))
				.arg(file)
				.status()
				.map(|status| status.success())
				.unwrap_or(false)
		};

		// 1. 优先连接 Neovim 传来的 ServerName
		if let Some(server_name) = config.vim_servername
			.as_deref()
				.filter(|s| !s.trim().is_empty())
		{
			if try_connect(server_name) {
				return;
			}
		}

		// 2. 使用 PDF 完整路径生成 Hash

		let pdf_hash = pdf_ipc_hash(pdf_path);
		let socket_path = format!(
			"/tmp/vectorsnap-nvim-{:016x}.sock",
			pdf_hash
		);

		// 3. 尝试连接 PDF Hash 对应的 Neovim
		if Path::new(&socket_path).exists() {
			if try_connect(&socket_path) {
				return;
			}

			// 连接失败，清理失效 socket
			let _ = std::fs::remove_file(&socket_path);
		}

		// 4. 启动新的 Neovim 实例
		spawn_and_reap(
			Command::new(raw_cmd)
			.arg("--listen")
			.arg(&socket_path)
			.arg(format!("+{}", line))
			.arg(file)
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			);
	}
//open neovide
//impl PdfApp {
//	fn open_neovide_from_synctex(
//		&self,
//		raw_cmd: &str,
//		file: &str,
//		line: &str,
//	) {
//		let pdf_path = match &self.pdf_path {
//			Some(p) => p,
//			None => return,
//		};
//
//		//let escaped_file = file.replace('\\', "\\\\").replace('\'', "\\'");
//		//let expr = format!(:w
//		//	"execute('edit ' . fnameescape('{}')) | execute('{}')",
//		//	escaped_file,
//		//	line
//		//);
//
//		let escaped_file = file.replace('\'', "''");
//		let line_number = match line.trim().parse::<u32>() {
//			Ok(n) => n,
//			Err(_) => return,
//		};
//		let expr = format!("v:lua.jump_to_line('{}', {})", escaped_file, line_number);
//		let nvim_cmd = &self.config.nvim_path;
//
//		let try_connect = |server: &str| -> Result<(), bool> {
//			match Command::new(nvim_cmd)
//				.arg("--server")
//				.arg(server)
//				.arg("--remote-expr")
//				.arg(&expr)
//				.output()
//				{
//					Ok(output) => {
//						if output.status.success() { return Ok(()); }
//
//						let stderr = String::from_utf8_lossy(&output.stderr);
//
//						if stderr.contains("E247") || stderr.contains("connection refused") {
//							Err(true)
//						} else {
//							Err(false)
//						}
//					}
//					Err(_) => Err(false),
//				}
//		};
//
//		// 优先连接 Neovide 传来的 ServerName
//		if let Some(server_name) = self.vim_servername.as_deref().filter(|s| !s.trim().is_empty()) {
//			match try_connect(server_name) {
//				Ok(()) => return,
//				Err(false) => return,
//				Err(true) => {}
//			}
//		}
//
//		// 使用 PDF Hash 作为备用标识
//		//let normalized_path = pdf_path
//		//	.canonicalize()
//		//	.unwrap_or_else(|_| pdf_path.to_path_buf());
//
//		//let path_str = normalized_path.to_string_lossy();
//
//		//let mut pdf_hash: u64 = 0xcbf29ce484222325;
//
//		//for byte in path_str.bytes() {
//		//	pdf_hash ^= byte as u64;
//		//	pdf_hash = pdf_hash.wrapping_mul(0x100000001b3);
//		//}
//
//		let pdf_hash = pdf_ipc_hash(pdf_path);
//		let socket_path = format!("/tmp/vectorsnap-nvim-{:016x}.sock", pdf_hash);
//
//		if Path::new(&socket_path).exists() {
//			match try_connect(&socket_path) {
//				Ok(()) => return,
//				Err(false) => return,
//				Err(true) => {
//					let _ = std::fs::remove_file(&socket_path);
//				}
//			}
//		}
//
//		//// 启动新的 Neovide 实例
//		spawn_and_reap(
//			Command::new(raw_cmd)
//			.arg(file)
//			.arg("--")
//			.arg("--listen")
//			.arg(&socket_path)
//			.arg(format!("+{}", line))
//			.spawn();
//			);
//	}
//}
	fn open_neovide_from_synctex(
		config: &SynctexEditorConfig,
		raw_cmd: &str,
		file: &str,
		line: &str,
	) {

		let pdf_path = &config.pdf_path;

    let nvim_cmd = if config.nvim_path.trim().is_empty() {
        "nvim"
    } else {
        config.nvim_path.trim()
    };

		//let escaped_file = file.replace('\\', "\\\\").replace('\'', "\\'");
		//let expr = format!(:w
		//	"execute('edit ' . fnameescape('{}')) | execute('{}')",
		//	escaped_file,
		//	line
		//);

		let escaped_file = file.replace('\'', "''");
		let line_number = match line.trim().parse::<u32>() {
			Ok(n) => n,
			Err(_) => return,
		};
		let expr = format!("v:lua.jump_to_line('{}', {})", escaped_file, line_number);

		let try_connect = |server: &str| -> Result<(), bool> {
			match Command::new(nvim_cmd)
				.arg("--server")
				.arg(server)
				.arg("--remote-expr")
				.arg(&expr)
				.output()
				{
					Ok(output) => {
						if output.status.success() { return Ok(()); }

						let stderr = String::from_utf8_lossy(&output.stderr);

						if stderr.contains("E247") || stderr.contains("connection refused") {
							Err(true)
						} else {
							Err(false)
						}
					}
					Err(_) => Err(false),
				}
		};

		// 优先连接 Neovide 传来的 ServerName
		if let Some(server_name) = config.vim_servername
			.as_deref().filter(|s| !s.trim().is_empty()) {
			match try_connect(server_name) {
				Ok(()) => return,
				Err(false) => return,
				Err(true) => {}
			}
		}

		// 使用 PDF Hash 作为备用标识
		let pdf_hash = pdf_ipc_hash(pdf_path);
		let socket_path = format!("/tmp/vectorsnap-nvim-{:016x}.sock", pdf_hash);

		if Path::new(&socket_path).exists() {
			match try_connect(&socket_path) {
				Ok(()) => return,
				Err(false) => return,
				Err(true) => {
					let _ = std::fs::remove_file(&socket_path);
				}
			}
		}

		//// 启动新的 Neovide 实例
		spawn_and_reap(
			Command::new(raw_cmd)
			.arg(file)
			.arg("--")
			.arg("--listen")
			.arg(&socket_path)
			.arg(format!("+{}", line))
			.spawn()
			);
	}
//open custom editor
//impl PdfApp {
//	fn open_custom_editor_from_synctex(
//		&self,
//		raw_cmd: &str,
//		file: &str,
//		line: &str,
//	) {
//		//    let has_quotes =
//		//        raw_cmd.contains("\"{file}\"")
//		//        || raw_cmd.contains("'{file}'");
//
//		//    let safe_file = if has_quotes {
//		//        file.to_string()
//		//    } else {
//		//        format!("\"{}\"", file)
//		//    };
//
//		//    let final_command = raw_cmd
//		//        .replace("{file}", &safe_file)
//		//        .replace("{line}", line);
//		let final_command = raw_cmd
//			.replace("{file}", file)
//			.replace("{line}", line);
//
//		// Linux
//		#[cfg(target_os = "linux")]
//		{
//			spawn_and_reap(Command::new("sh").arg("-c").arg(&final_command).spawn(););
//		}
//
//		// Windows
//		#[cfg(target_os = "windows")]
//		{
//			use std::os::windows::process::CommandExt;
//
//			const CREATE_NO_WINDOW: u32 = 0x08000000;
//			spawn_and_reap(
//				Command::new("cmd")
//				.arg("/C")
//				.arg(&final_command)
//				.creation_flags(CREATE_NO_WINDOW)
//				.spawn();
//			);
//		}
//	}
//}
	fn open_custom_editor_from_synctex(
		raw_cmd: &str,
		file: &str,
		line: &str,
	) {
		//    let has_quotes =
		//        raw_cmd.contains("\"{file}\"")
		//        || raw_cmd.contains("'{file}'");

		//    let safe_file = if has_quotes {
		//        file.to_string()
		//    } else {
		//        format!("\"{}\"", file)
		//    };

		//    let final_command = raw_cmd
		//        .replace("{file}", &safe_file)
		//        .replace("{line}", line);
		let final_command = raw_cmd
			.replace("{file}", file)
			.replace("{line}", line);

		// Linux
		#[cfg(target_os = "linux")]
		{
			spawn_and_reap(Command::new("sh").arg("-c").arg(&final_command).spawn());
		}

		// Windows
		#[cfg(target_os = "windows")]
		{
			use std::os::windows::process::CommandExt;

			const CREATE_NO_WINDOW: u32 = 0x08000000;
			spawn_and_reap(
				Command::new("cmd")
				.arg("/C")
				.arg(&final_command)
				.creation_flags(CREATE_NO_WINDOW)
				.spawn()
			);
		}
	}
