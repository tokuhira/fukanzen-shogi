/// ポータルメニュー — 単体検証卓・通信対戦の選択と接続設定
use std::io;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

use engine::types::Side;
use crate::online::{ConnectMode, OnlineConfig};

pub enum PortalResult {
    Local,
    Online(OnlineConfig),
    Quit,
}

enum Screen {
    Menu { selected: usize },
    OnlineForm {
        listen: bool,
        addr_or_port: String,
        secret: String,
        focused: usize, // 0 = addr/port フィールド, 1 = secret フィールド
        error: Option<String>,
    },
}

const MENU_LABELS: &[&str] = &[
    "ローカル検証卓",
    "通信対戦（先手・待ち受け）",
    "通信対戦（後手・接続）",
    "終了",
];

pub fn run_portal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<PortalResult> {
    let mut screen = Screen::Menu { selected: 0 };

    loop {
        terminal.draw(|f| render(f, &screen))?;

        match event::read()? {
            Event::Key(key) => {
                // Ctrl+C は常に終了
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('c')
                {
                    return Ok(PortalResult::Quit);
                }

                let mut next_screen: Option<Screen> = None;
                let mut portal_result: Option<PortalResult> = None;

                match &mut screen {
                    Screen::Menu { selected } => match key.code {
                        KeyCode::Up => {
                            if *selected > 0 {
                                *selected -= 1;
                            }
                        }
                        KeyCode::Down => {
                            if *selected < MENU_LABELS.len() - 1 {
                                *selected += 1;
                            }
                        }
                        KeyCode::Enter | KeyCode::Char(' ') => match *selected {
                            0 => portal_result = Some(PortalResult::Local),
                            1 => next_screen = Some(Screen::OnlineForm {
                                listen: true,
                                addr_or_port: String::new(),
                                secret: String::new(),
                                focused: 0,
                                error: None,
                            }),
                            2 => next_screen = Some(Screen::OnlineForm {
                                listen: false,
                                addr_or_port: String::new(),
                                secret: String::new(),
                                focused: 0,
                                error: None,
                            }),
                            _ => portal_result = Some(PortalResult::Quit),
                        },
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            portal_result = Some(PortalResult::Quit);
                        }
                        _ => {}
                    },

                    Screen::OnlineForm {
                        listen,
                        addr_or_port,
                        secret,
                        focused,
                        error,
                    } => match key.code {
                        KeyCode::Esc => {
                            next_screen = Some(Screen::Menu { selected: 0 });
                        }
                        KeyCode::Tab | KeyCode::Down => {
                            *focused = 1 - *focused;
                        }
                        KeyCode::Up => {
                            *focused = 1 - *focused;
                        }
                        KeyCode::Backspace => {
                            if *focused == 0 {
                                addr_or_port.pop();
                            } else {
                                secret.pop();
                            }
                            *error = None;
                        }
                        KeyCode::Enter => {
                            if *focused == 0 {
                                *focused = 1;
                            } else {
                                match try_submit(*listen, addr_or_port, secret) {
                                    Ok(config) => {
                                        portal_result = Some(PortalResult::Online(config));
                                    }
                                    Err(msg) => {
                                        *error = Some(msg);
                                        *focused = 0;
                                    }
                                }
                            }
                        }
                        KeyCode::Char(c) => {
                            if *focused == 0 {
                                addr_or_port.push(c);
                            } else {
                                secret.push(c);
                            }
                            *error = None;
                        }
                        _ => {}
                    },
                }

                if let Some(s) = next_screen {
                    screen = s;
                }
                if let Some(r) = portal_result {
                    return Ok(r);
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

fn try_submit(listen: bool, addr_or_port: &str, secret: &str) -> Result<OnlineConfig, String> {
    let mode = if listen {
        let port = addr_or_port.trim().parse::<u16>().map_err(|_| {
            "ポート番号は 1〜65535 の整数で入力してください".to_string()
        })?;
        ConnectMode::Listen(port)
    } else {
        let addr = addr_or_port.trim();
        if addr.is_empty() {
            return Err("接続先アドレスを入力してください (例: 192.168.1.10:8765)".to_string());
        }
        ConnectMode::Connect(addr.to_string())
    };
    let local_side = if listen { Side::Sente } else { Side::Gote };
    Ok(OnlineConfig {
        local_side,
        mode,
        secret: secret.as_bytes().to_vec(),
    })
}

// ─── 描画 ────────────────────────────────────────────────────────────────────

fn render(f: &mut Frame, screen: &Screen) {
    match screen {
        Screen::Menu { selected } => render_menu(f, *selected),
        Screen::OnlineForm { listen, addr_or_port, secret, focused, error } => {
            render_form(f, *listen, addr_or_port, secret, *focused, error.as_deref());
        }
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

fn render_menu(f: &mut Frame, selected: usize) {
    let area = centered_rect(46, 12, f.area());

    let block = Block::default()
        .title(" 不完全将棋 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // spacer / item0 / item1 / item2 / spacer / item3(終了) / spacer / help
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    let item_rows = [chunks[1], chunks[2], chunks[3], chunks[5]];

    for (i, (label, row)) in MENU_LABELS.iter().zip(item_rows.iter()).enumerate() {
        let (cursor, style) = if i == selected {
            (
                "▶ ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            ("  ", Style::default().fg(Color::Gray))
        };
        let line = Line::from(vec![Span::raw(cursor), Span::styled(*label, style)]);
        f.render_widget(Paragraph::new(line), *row);
    }

    f.render_widget(
        Paragraph::new(Span::styled(
            "[↑↓] 選択  [Enter] 決定  [q] 終了",
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(Alignment::Center),
        chunks[7],
    );
}

fn render_form(
    f: &mut Frame,
    listen: bool,
    addr_or_port: &str,
    secret: &str,
    focused: usize,
    error: Option<&str>,
) {
    let area = centered_rect(52, 11, f.area());

    let title = if listen {
        " 通信対戦（先手・待ち受け） "
    } else {
        " 通信対戦（後手・接続） "
    };

    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // spacer / label1 / input1 / spacer / label2 / input2 / error / spacer / help
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    // フィールド1
    let label1 = if listen { "待ち受けポート番号:" } else { "接続先 (host:port):" };
    f.render_widget(
        Paragraph::new(Span::styled(label1, Style::default().fg(Color::Gray))),
        chunks[1],
    );
    let (f1_text, f1_style) = input_display(addr_or_port, focused == 0);
    f.render_widget(Paragraph::new(Span::styled(f1_text, f1_style)), chunks[2]);

    // フィールド2
    f.render_widget(
        Paragraph::new(Span::styled("共有パスワード:", Style::default().fg(Color::Gray))),
        chunks[4],
    );
    let masked = "*".repeat(secret.len());
    let (f2_text, f2_style) = input_display(&masked, focused == 1);
    f.render_widget(Paragraph::new(Span::styled(f2_text, f2_style)), chunks[5]);

    // エラーメッセージ
    if let Some(msg) = error {
        f.render_widget(
            Paragraph::new(Span::styled(msg, Style::default().fg(Color::Red))),
            chunks[6],
        );
    }

    f.render_widget(
        Paragraph::new(Span::styled(
            "[Tab/↑↓] 移動  [Enter] 次/開始  [Esc] 戻る",
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(Alignment::Center),
        chunks[8],
    );
}

fn input_display(value: &str, active: bool) -> (String, Style) {
    if active {
        (
            format!("[{}▌]", value),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            format!("[{}]", value),
            Style::default().fg(Color::DarkGray),
        )
    }
}
