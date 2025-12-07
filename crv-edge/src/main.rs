#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
use tokio::signal;

#[cfg(not(windows))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ctrl+C 优雅关闭触发器
    let shutdown = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        println!("\nReceived CTRL+C signal, shutting down gracefully...");
    };

    // 使用支持优雅关闭的启动函数
    crv_edge::daemon_server::startup::start_server_with_shutdown(shutdown).await
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use crv_edge::pb::BonjourReq;
    use crv_edge::pb::edge_daemon_service_client::EdgeDaemonServiceClient;
    use image::GenericImageView;
    use image::ImageReader;
    use std::time::Duration;
    use tao::event::Event;
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tokio::runtime::Builder as TokioRuntimeBuilder;
    use tokio::sync::oneshot;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, TrayIconBuilder};

    // 启动 Tokio 运行时与 gRPC 服务
    let runtime = TokioRuntimeBuilder::new_multi_thread()
        .enable_all()
        .build()?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server_handle = runtime.spawn(async move {
        let shutdown = async move {
            let _ = shutdown_rx.await;
        };
        let _ = crv_edge::daemon_server::startup::start_server_with_shutdown(shutdown).await;
    });

    // 创建托盘菜单
    let menu = Menu::new();
    let status_item = MenuItem::new("启动中...", true, None);
    let quit_item = MenuItem::new("退出", true, None);
    // 记录退出按钮的 id
    let quit_id = quit_item.id().clone();
    menu.append(&status_item)?;
    menu.append(&quit_item)?;

    // 加载内置 ICO 图标（支持多尺寸，选第一帧并缩放至 32x32）
    let icon_bytes: &[u8] = include_bytes!("../resources/icon.ico");
    let dyn_img = ImageReader::new(std::io::Cursor::new(icon_bytes))
        .with_guessed_format()?
        .decode()?;
    let rgba32 = dyn_img.to_rgba8();
    let (w, h) = dyn_img.dimensions();
    // tray-icon 需要固定尺寸；如不是 32x32，尝试缩放
    let (w32, h32) = (32u32, 32u32);
    let rgba32 = if w != w32 || h != h32 {
        image::imageops::resize(&rgba32, w32, h32, image::imageops::FilterType::Triangle)
    } else {
        rgba32
    };
    let icon = Icon::from_rgba(rgba32.into_raw(), w32 as u32, h32 as u32)?;

    // 自定义用户事件用于更新状态
    #[derive(Clone)]
    enum AppEvent {
        StatusRunning,
        StatusFailed,
    }

    // 事件循环
    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("crv-edge")
        .with_icon(icon)
        .build()?;

    // MenuEvent 监听（由 tray-icon 提供的全局事件接收器）
    let menu_event_rx = MenuEvent::receiver();

    // 将一次性通道和句柄包装在 Option 里，以便在闭包内安全地 take
    let mut shutdown_tx = Some(shutdown_tx);
    let mut server_handle = Some(server_handle);

    // 启动后进行一次自检：尝试连接 gRPC 并调用 bonjour
    let proxy_clone = proxy.clone();
    runtime.spawn(async move {
        // 稍等片刻让 server 绑定端口
        tokio::time::sleep(Duration::from_millis(300)).await;
        let endpoint = format!("http://{}:{}", "127.0.0.1", 34562);
        let result = async {
            let mut client = EdgeDaemonServiceClient::connect(endpoint).await.ok()?;
            let _ = client
                .bonjour(tonic::Request::new(BonjourReq {}))
                .await
                .ok()?;
            Some(())
        }
        .await;
        let _ = match result {
            Some(_) => proxy_clone.send_event(AppEvent::StatusRunning),
            None => proxy_clone.send_event(AppEvent::StatusFailed),
        };
    });

    // 为了在主循环内访问最新的菜单事件，使用 try_recv 读取（非阻塞）
    // 注意：MenuEvent::receiver() 是全局的，这里直接复用上面的 menu_event_rx 进行 try_recv
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        // 轮询菜单事件（非阻塞）
        while let Ok(menu_event) = menu_event_rx.try_recv() {
            if menu_event.id == quit_id {
                // 更新状态为“正在退出”
                let _ = status_item.set_text("正在退出");

                // 触发优雅关闭（仅一次）
                if let Some(tx) = shutdown_tx.take() {
                    let _ = tx.send(());
                }

                // 等待 gRPC 任务结束（仅一次）
                if let Some(handle) = server_handle.take() {
                    let _ = runtime.block_on(async {
                        let _ = handle.await;
                    });
                }

                *control_flow = ControlFlow::Exit;
                return;
            }
        }

        match event {
            Event::UserEvent(AppEvent::StatusRunning) => {
                let _ = status_item.set_text("正在运行");
            }
            Event::UserEvent(AppEvent::StatusFailed) => {
                let _ = status_item.set_text("启动失败");
            }
            Event::LoopDestroyed => {
                // 事件循环销毁时确保触发关闭
                if let Some(tx) = shutdown_tx.take() {
                    let _ = tx.send(());
                }
                if let Some(handle) = server_handle.take() {
                    let _ = runtime.block_on(async {
                        let _ = handle.await;
                    });
                }
            }
            _ => {}
        }
    });
}
