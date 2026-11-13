use std::path::{PathBuf};
use std::sync::mpsc;
use std::io::{Read, Write};
use std::thread;
use interprocess::local_socket::{prelude::*, ListenerOptions, Stream};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct IpcMessage {
    pub path: Option<PathBuf>,
    pub page: Option<usize>,
    pub servername: Option<String>,

    #[serde(default)]
    pub editor: Option<String>,

		#[serde(default)]
    pub synctex: Option<String>,
}

pub enum IpcStartup {
    Exit,
    Primary(
        mpsc::Receiver<IpcMessage>,
        IpcHandle,
    ),
}

pub struct IpcHandle {
    stop_tx: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
    ipc_id: String,
}

impl Drop for IpcHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());

        #[cfg(target_os = "windows")]
        {
            if let Ok(name) = self.ipc_id
                .as_str()
                .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
            {
                drop(Stream::connect(name));
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let socket_path = format!("/tmp/{}.sock", self.ipc_id);

            if let Ok(name) = socket_path
                .as_str()
                .to_fs_name::<interprocess::local_socket::GenericFilePath>()
            {
                drop(Stream::connect(name));
            }
        }

        // 不等待可能阻塞在 read_to_string() 的线程
        self.thread.take();
    }
}

pub fn setup_ipc(hash: u64, message: IpcMessage) -> IpcStartup {
	let ipc_id = format!("vectorsnap_{:016x}", hash);

	// Windows 使用命名管道名称
	#[cfg(target_os = "windows")]
	let ipc_name = match ipc_id
		.as_str()
		.to_ns_name::<interprocess::local_socket::GenericNamespaced>()
		{
			Ok(name) => name,
			Err(_) => { return IpcStartup::Exit; }
		};

	// Linux / macOS 使用 Unix Socket 文件
	#[cfg(not(target_os = "windows"))]
	let socket_path = format!("/tmp/{}.sock", ipc_id);

	#[cfg(not(target_os = "windows"))]
	let ipc_name = match socket_path
		.as_str()
		.to_fs_name::<interprocess::local_socket::GenericFilePath>()
		{
			Ok(name) => name,
			Err(_) => { return IpcStartup::Exit; }
		};

	// 1. 尝试连接已经运行的实例
	match Stream::connect(ipc_name.clone()) {
		Ok(mut conn) => {
			if let Ok(payload) = serde_json::to_string(&message) {
				let _ = conn.write_all(payload.as_bytes());
			}
			drop(conn);
			return IpcStartup::Exit;
		}

		Err(_) => {
			#[cfg(not(target_os = "windows"))]
			{
				if std::path::Path::new(&socket_path).exists() {
					let _ = std::fs::remove_file(&socket_path);
				}
			}
		}
	}

	// 2. 创建 Listener，成为当前 PDF 的主实例
	let listener = match ListenerOptions::new()
		.name(ipc_name)
		.create_sync()
		{
			Ok(l) => l,
			Err(_) => { return IpcStartup::Exit; }
		};

	// 3. 创建消息通道和停止通道
	let (tx, rx) = mpsc::channel::<IpcMessage>();
	let (stop_tx, stop_rx) = mpsc::channel::<()>();

	// 4. 启动监听线程
	let thread = std::thread::spawn(move || {
		// 只创建一次 incoming 迭代器
		let mut incoming = listener.incoming();

		loop {
			match incoming.next() {
				Some(Ok(mut conn)) => {
					let mut buf = String::new();

					if conn.read_to_string(&mut buf).is_ok() && !buf.is_empty() {
						if let Ok(msg) = serde_json::from_str::<IpcMessage>(&buf) {
							if msg.path.is_some() 
								|| msg.page.is_some() 
									|| msg.servername.is_some() 
									|| msg.editor.is_some()
									|| msg.synctex.is_some()
							{
								let _ = tx.send(msg);
							}
						}
					}

					// 当前连接处理完毕，再检查停止信号
					match stop_rx.try_recv() {
						Ok(()) | Err(mpsc::TryRecvError::Disconnected) => { break; }
						Err(mpsc::TryRecvError::Empty) => {}
					}
				}

				Some(Err(_)) => {
					match stop_rx.try_recv() {
						Ok(()) | Err(mpsc::TryRecvError::Disconnected) => { break; }
						Err(mpsc::TryRecvError::Empty) => {}
					}
				}

				None => break,
			}
		}

	});

	// 5. 保存监听线程生命周期
	let handle = IpcHandle {stop_tx, thread: Some(thread), ipc_id};

	IpcStartup::Primary(rx, handle)
}
pub fn pdf_ipc_hash(path: &std::path::Path) -> u64 {
    // 确保使用规范化的绝对路径
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_str = path.to_string_lossy();

    // 使用 64 位 FNV-1a 确定性哈希算法
    // 该算法不依赖进程随机种子，确保相同的路径在任何时间、任何重启下算出的 Hash 绝对一致
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in path_str.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}


//// 统一解析命令行参数的辅助函数
pub fn parse_args(
    args: &[String],
) -> (
    Option<std::path::PathBuf>,
    Option<usize>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let mut pdf_path = None;
    let mut page_override = None;
    let mut servername = None;
    let mut editor = None;
    let mut synctex: Option<String> = None;

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            _ if arg.starts_with("--page=") => {
                if let Some(value) = arg.strip_prefix("--page=") {
                    if let Ok(p) = value.parse::<usize>() {
                        page_override = Some(p.saturating_sub(1));
                    }
                }
            }

            _ if arg.starts_with("--servername=") => {
                servername = arg
                    .strip_prefix("--servername=")
                    .map(|v| v.to_string());
            }

            _ if arg.starts_with("--editor=") => {
                editor = arg
                    .strip_prefix("--editor=")
                    .map(|v| v.to_string());
            }

            _ if arg.starts_with("--synctex=") => {
                synctex = arg
                    .strip_prefix("--synctex=")
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty());
            }

            _ if !arg.starts_with("--") && pdf_path.is_none() => {
                let path = std::path::PathBuf::from(arg);

                let abs_path = if path.is_absolute() {
                    path
                } else {
                    std::env::current_dir()
                        .map(|mut cur| {
                            cur.push(&path);
                            cur
                        })
                        .unwrap_or(path)
                };

                let final_path = abs_path.canonicalize().unwrap_or(abs_path);
                pdf_path = Some(final_path);
            }

            _ => {}
        }
    }

    (
        pdf_path,
        page_override,
        servername,
        editor,
        synctex,
    )
}
