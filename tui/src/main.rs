/// 不完全将棋 TUI 検証卓（第二段階）
///
/// ratatui + crossterm による全画面 TUI。
/// 一人が先手・後手の両着手をカーソル/マウスで組み立て、同時解決する検証モード。
/// エンジンクレートの公開 API のみを使用し、engine/ は無改変。
use std::io::{self, Stdout};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

mod app;
mod input;
mod ui;

use app::App;

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

    let mut app = App::new();
    let result = run_app(&mut terminal, &mut app);

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

fn run_app(
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
            Event::Resize(_, _) => {
                // 次ループで再描画するので何もしない
            }
            _ => {}
        }
    }
}
