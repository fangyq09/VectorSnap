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
		vim_function: Option<String>,
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
			vim_function: self.config.vim_function.clone(),
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
		let mut cmd = Command::new("synctex");

		cmd.arg("edit")
			.arg("-o")
			.arg(format!("{}:{}:{}:{}", page_index + 1, target_x, target_y, path.display()));

		// 如果配置了 synctex_dir，则指定 SyncTeX 文件目录
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

		thread::spawn(move || {
			let output = cmd.output();
			let Ok(out) = output else { return; };
			if !out.status.success() { return; }
			let result = String::from_utf8_lossy(&out.stdout).into_owned();
			open_editor_from_synctex(&editor_config, &result);
		});
	}
}
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
fn open_gvim_from_synctex(config: &SynctexEditorConfig, raw_cmd: &str, file: &str, line: &str) {
    let pdf_path = &config.pdf_path;
    // -----------------------
    let server_name = config.vim_servername
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            pdf_path
                .to_string_lossy()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
                .collect::<String>()
        });
    // 行号只允许数字,避免被注入到 Ex 命令里
    let line = if !line.is_empty() && line.chars().all(|c| c.is_ascii_digit()) {
        line
    } else {
        "1"
    };

    // 函数名校验(与 execute_vim_function 规则一致)
    let function = config.vim_function
        .as_deref()
        .map(str::trim)
        .filter(|f| {
            !f.is_empty()
                && f.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '#')
        });

    // +{cmd}:先跳到行号,再调用函数
    let plus_cmd = match function {
        Some(f) => format!("+{}|call {}()", line, f),
        None => format!("+{}", line),
    };

    spawn_and_reap(
        Command::new(raw_cmd)
            .arg("--servername")
            .arg(&server_name)
            .arg("--remote-silent")
            .arg(plus_cmd)
            .arg(file)
            .spawn()
    );
}
//open custom editor
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

//    //let vim_command = format!(":<C-u>call {}()<CR>", function);
//    //command.arg("--remote-send").arg(vim_command).status().map(|status| status.success()).unwrap_or(false)
//		let expression = format!("execute('call {}()')", function);
//		command.arg("--remote-expr").arg(expression).status().map(|status| status.success()).unwrap_or(false)

// 行号只允许纯数字,否则退回第 1 行(防止被注入 Ex 命令)
fn sanitize_line(line: &str) -> &str {
    let line = line.trim();
    if !line.is_empty() && line.chars().all(|c| c.is_ascii_digit()) {
        line
    } else {
        "1"
    }
}

// 取出合法的 Vim 函数名
fn valid_vim_function(config: &SynctexEditorConfig) -> Option<&str> {
    config.vim_function
        .as_deref()
        .map(str::trim)
        .filter(|f| {
            !f.is_empty()
                && f.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '#')
        })
}

// 生成 "+42|call Func()" 或 "+42"
fn jump_cmd(line: &str, function: Option<&str>) -> String {
    match function {
        Some(f) => format!("+{}|call {}()", line, f),
        None => format!("+{}", line),
    }
}

// open nvim
fn open_nvim_from_synctex(
    config: &SynctexEditorConfig,
    raw_cmd: &str,
    file: &str,
    line: &str,
) {
    let pdf_path = &config.pdf_path;
    let plus_cmd = jump_cmd(sanitize_line(line), valid_vim_function(config));

    // 尝试连接已有的 Neovim 实例:跳转 + 调用函数一次完成
    let try_connect = |server: &str| -> bool {
        Command::new(raw_cmd)
            .arg("--server")
            .arg(server)
            .arg("--remote")
            .arg(&plus_cmd)
            .arg(file)
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    };

    // 1. 优先连接传来的 ServerName
    if let Some(server_name) = config.vim_servername
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        if try_connect(server_name) {
            return;
        }
    }

    // 2. 使用 PDF Hash 对应的 socket
    let pdf_hash = pdf_ipc_hash(pdf_path);
    let socket_path = format!("/tmp/vectorsnap-nvim-{:016x}.sock", pdf_hash);

    if Path::new(&socket_path).exists() {
        if try_connect(&socket_path) {
            return;
        }
        // 连接失败,清理失效 socket
        let _ = std::fs::remove_file(&socket_path);
    }

    // 3. 没有可连接的实例,启动新的 Neovim(启动时就执行 +cmd)
    spawn_and_reap(
        Command::new(raw_cmd)
            .arg("--listen")
            .arg(&socket_path)
            .arg(&plus_cmd)
            .arg(file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    );
}
//open neovide
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

    let line_number = match line.trim().parse::<u32>() {
        Ok(n) => n,
        Err(_) => return,
    };
    let line_str = line_number.to_string();
    let function = valid_vim_function(config);

    // 已运行实例:一个表达式里先跳转、再调用函数(列表元素按顺序求值)
    let escaped_file = file.replace('\'', "''");
    let expr = match function {
        Some(f) => format!(
            "[v:lua.jump_to_line('{}', {}), execute('call {}()')][0]",
            escaped_file, line_number, f
        ),
        None => format!("v:lua.jump_to_line('{}', {})", escaped_file, line_number),
    };

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
        .as_deref().filter(|s| !s.trim().is_empty())
    {
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

    // 启动新的 Neovide 实例:启动时直接带 +cmd
    spawn_and_reap(
        Command::new(raw_cmd)
            .arg(file)
            .arg("--")
            .arg("--listen")
            .arg(&socket_path)
            .arg(jump_cmd(&line_str, function))
            .spawn()
    );
}
//==================================================
//// 根据 synctex 正向搜索参数解析出 PDF 页码(0-based)
////
//// synctex_query 格式: "行:列:光标所在的tex文件[|编译目录]"
////   - tex文件: 光标所在的 .tex(分章节写作时是子文件,不是主文件);
////              建议绝对路径,相对路径以 PDF 所在目录为基准;留空则回退为 PDF 同名 .tex
////   - 编译目录: 可选。省略 "|" 或 "|" 后为空 → 认为编译产物(.synctex.gz 等)
////              与 PDF、main.tex 同目录,不加 -d,让 synctex 就地查找;
////              非空 → 作为 -d 传入(绝对路径,或相对 PDF 所在目录)
////   用 '|' 分隔编译目录:Windows 路径自带 ':'(C:\...),两个路径字段不能再用 ':' 分隔
//pub fn resolve_synctex_page(pdf_path: &Path, synctex_query: &str) -> Option<usize> {
//    // splitn(3): 前两段是纯数字,剩余整体保留,
//    // tex文件/编译目录里的 ':'(如 Windows 的 C:...)不会被切断
//    let mut parts = synctex_query.trim().splitn(3, ':');
//    let line = parts.next()?.trim();
//    let col = parts.next()?.trim();
//    let rest = parts.next().unwrap_or("").trim();
//
//    if line.is_empty()
//        || !line.chars().all(|c| c.is_ascii_digit())
//        || !col.chars().all(|c| c.is_ascii_digit())
//    {
//        return None;
//    }
//
//    let base_dir = pdf_path.parent().unwrap_or(Path::new("."));
//    let resolve = |p: &str| -> PathBuf {
//        let p = PathBuf::from(p);
//        if p.is_absolute() { p } else { base_dir.join(p) }
//    };
//
//    // rest = "tex文件" 或 "tex文件|编译目录"
//    let (file_str, dir_str) = match rest.split_once('|') {
//        Some((f, d)) => (f.trim(), d.trim()),
//        None => (rest, ""),
//    };
//
//    // ★ 光标所在的真实源文件;留空时回退为 PDF 同名 .tex
//    let tex_path = if file_str.is_empty() {
//        pdf_path.with_extension("tex")
//    } else {
//        resolve(file_str)
//    };
//
//    let mut cmd = Command::new("synctex");
//    cmd.arg("view")
//        .arg("-i")
//        .arg(format!("{}:{}:{}", line, col, tex_path.display()))
//        .arg("-o")
//        .arg(pdf_path);
//
//    // 编译目录非空才加 -d;省略/为空 → 产物与 PDF 同目录,synctex 就地查找
//    if !dir_str.is_empty() {
//        cmd.arg("-d").arg(resolve(dir_str));
//    }
//
//    let output = cmd.output().ok()?;
//    if !output.status.success() {
//        eprintln!(
//            "synctex view failed: {}",
//            String::from_utf8_lossy(&output.stderr).trim()
//        );
//        return None;
//    }
//    let stdout = String::from_utf8_lossy(&output.stdout);
//    for l in stdout.lines() {
//        if let Some(page_str) = l.strip_prefix("Page:") {
//            if let Ok(p) = page_str.trim().parse::<usize>() {
//                return Some(p.saturating_sub(1)); // 转 0-based
//            }
//        }
//    }
//    None
//}
//--------------------------------------------------
//更加坚固的版本，能够识别\input{chaN.tex}这种结构
//--------------------------------------------------
// 运行 synctex view,解析出 0-based 页码
fn synctex_view_page(
    pdf_path: &Path,
    tex_path: &Path,
    line: &str,
    col: &str,
    dir: Option<&Path>,
    cwd: Option<&Path>,
) -> Option<usize> {
    let mut cmd = Command::new("synctex");
    cmd.arg("view")
        .arg("-i")
        .arg(format!("{}:{}:{}", line, col, tex_path.display()))
        .arg("-o")
        .arg(pdf_path);
    if let Some(d) = dir { cmd.arg("-d").arg(d); }
    if let Some(c) = cwd { cmd.current_dir(c); }

    let output = cmd.output().ok()?;
    if !output.status.success() {
        eprintln!("synctex view failed: {}", String::from_utf8_lossy(&output.stderr).trim());
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for l in stdout.lines() {
        if let Some(p) = l.strip_prefix("Page:") {
            if let Ok(p) = p.trim().parse::<usize>() {
                return Some(p.saturating_sub(1));
            }
        }
    }
    None
}

// 若 tex_path 第 line_no 行是 \input/\include/\subfile{X},解析出 X 对应的真实文件
fn input_target_at(tex_path: &Path, line_no: usize, base_dir: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(tex_path).ok()?;
    let raw = content.lines().nth(line_no.checked_sub(1)?)?;
    let line = raw.split('%').next().unwrap_or("").trim(); // 去掉注释,避免命中被注释掉的 \input
    for cmd in ["\\input", "\\include", "\\subfile"] {
        let Some(rest) = line.strip_prefix(cmd) else { continue };
        // 命令后必须是 '{' 或空白,避免误匹配 \includeonly 等更长的命令
        if rest.starts_with(|c: char| c.is_ascii_alphabetic()) { continue; }
        let rest = rest.trim_start();
        let name = if let Some(braced) = rest.strip_prefix('{') {
            match braced.find('}') {
                Some(end) => braced[..end].trim(),
                None => continue,
            }
        } else {
            rest.split_whitespace().next().unwrap_or("") // \input file.tex 裸写法
        };
        if name.is_empty() { continue; }
        if let Some(p) = resolve_input_target(tex_path, base_dir, name) {
            return Some(p);
        }
    }
    None
}

// 把 \input 的参数解析成存在的 .tex(相对主文件目录 / PDF 目录,可省略 .tex)
fn resolve_input_target(parent_tex: &Path, base_dir: &Path, name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() { return None; }
    let mut names = vec![name.to_string()];
    if !name.ends_with(".tex") { names.push(format!("{}.tex", name)); }
    let mut dirs: Vec<&Path> = Vec::new();
    if let Some(p) = parent_tex.parent() { dirs.push(p); }
    dirs.push(base_dir);
    for d in dirs {
        for n in &names {
            let p = d.join(n); // n 为绝对路径时 join 会直接替换
            if p.is_file() { return Some(p); }
        }
    }
    None
}

// synctex_query 格式: "行:列:光标所在的tex文件[|编译目录]"
pub fn resolve_synctex_page(pdf_path: &Path, synctex_query: &str) -> Option<usize> {
    let mut parts = synctex_query.trim().splitn(3, ':');
    let line = parts.next()?.trim();
    let col = parts.next()?.trim();
    let rest = parts.next().unwrap_or("").trim();
    if line.is_empty()
        || !line.chars().all(|c| c.is_ascii_digit())
        || !col.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }

    let base_dir = pdf_path.parent().unwrap_or(Path::new("."));
    let resolve = |p: &str| -> PathBuf {
        let p = PathBuf::from(p);
        if p.is_absolute() { p } else { base_dir.join(p) }
    };
    let (file_str, dir_str) = match rest.split_once('|') {
        Some((f, d)) => (f.trim(), d.trim()),
        None => (rest, ""),
    };
    let tex_path = if file_str.is_empty() {
        pdf_path.with_extension("tex")
    } else {
        resolve(file_str)
    };
    let build_dir = if dir_str.is_empty() { None } else { Some(resolve(dir_str)) };

    // ★ 关键:光标停在 \input/\include 行时,沿链重定向到真正产生内容的文件第 1 行
    let mut q_tex = tex_path.clone();
    let mut q_line = line.to_string();
    let mut q_col = col.to_string();
    for _ in 0..8 {
        let Ok(n) = q_line.parse::<usize>() else { break };
        match input_target_at(&q_tex, n, base_dir) {
            Some(t) => { q_tex = t; q_line = "1".to_string(); q_col = "1".to_string(); }
            None => break,
        }
    }
    if let Some(p) = synctex_view_page(pdf_path, &q_tex, &q_line, &q_col, build_dir.as_deref(), None) {
        return Some(p);
    }

    // 重定向没查到 → 退回原始文件/行
    if q_tex != tex_path {
        if let Some(p) = synctex_view_page(pdf_path, &tex_path, line, col, build_dir.as_deref(), None) {
            return Some(p);
        }
    }

    // 兜底:相对 PDF 目录的路径 + 以该目录为 cwd(适配记录相对路径的 synctex)
    if let Ok(rel) = tex_path.strip_prefix(base_dir) {
        if let Some(p) = synctex_view_page(pdf_path, rel, line, col, build_dir.as_deref(), Some(base_dir)) {
            return Some(p);
        }
    }
    None
}
//==================================================
