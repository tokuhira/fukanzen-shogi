/// 不完全将棋 TUI（第三段階: 通信秘匿対戦対応）
///
/// 引数なし          → ローカル検証モード（先後を1人が操作）
/// --listen PORT      → 先手として PORT で接続待ち
/// --connect ADDR     → 後手として ADDR (host:port) へ接続
/// --secret SECRET    → 共有パスワード（通信モード時に必須）
use std::io::{self, Stdout};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

mod app;
mod input;
mod net;
mod online;
mod ui;

use app::App;
use online::{ConnectMode, OnlineConfig};

fn main() -> io::Result<()> {
    // パニック時も端末を復元する
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        default_hook(info);
    }));

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal);

    restore_terminal()?;

    if let Err(ref e) = result {
        eprintln!("エラー: {}", e);
    }
    result
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if let Some(config) = parse_online_args(&args) {
        online::run_online(terminal, config)
    } else {
        let mut app = App::new();
        run_local(terminal, &mut app)
    }
}

fn parse_online_args(args: &[String]) -> Option<OnlineConfig> {
    use engine::types::Side;

    let mut listen_port: Option<u16> = None;
    let mut connect_addr: Option<String> = None;
    let mut secret: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--listen" => {
                i += 1;
                if let Some(s) = args.get(i) {
                    listen_port = s.parse().ok();
                }
            }
            "--connect" => {
                i += 1;
                connect_addr = args.get(i).cloned();
            }
            "--secret" => {
                i += 1;
                secret = args.get(i).cloned();
            }
            _ => {}
        }
        i += 1;
    }

    if listen_port.is_none() && connect_addr.is_none() {
        return None;
    }

    let secret_bytes = secret.unwrap_or_default().into_bytes();
    if secret_bytes.is_empty() {
        eprintln!("警告: --secret が指定されていません。空のパスワードで接続します。");
    }

    if let Some(port) = listen_port {
        Some(OnlineConfig {
            local_side: Side::Sente,
            mode: ConnectMode::Listen(port),
            secret: secret_bytes,
        })
    } else if let Some(addr) = connect_addr {
        Some(OnlineConfig {
            local_side: Side::Gote,
            mode: ConnectMode::Connect(addr),
            secret: secret_bytes,
        })
    } else {
        None
    }
}

fn run_local(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        match event::read()? {
            Event::Key(key) => {
                if input::handle_key(key, app) {
                    return Ok(());
                }
            }
            Event::Mouse(mouse) => {
                if input::handle_mouse(mouse, app) {
                    return Ok(());
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}
