use bcshot_common::{Config, ImageFormat, Status};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction},
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    switch::Switch,
    v_flex,
};
use gpui_kit::{prelude::*, *};
use std::{path::PathBuf, process::Command, time::Duration};

/// The numeric fields, in the order of `SettingsView::nums`.
const NUM_LABELS: [&str; 6] = ["X", "Y", "Width", "Height", "Screen width", "Screen height"];
const MIN_PX: u32 = 8;

pub struct SettingsView {
    cfg: Config,
    cfg_mtime: Option<std::time::SystemTime>,
    nums: Vec<Entity<InputState>>,
    prefix: Entity<InputState>,
    quality: Entity<SliderState>,
    /// None = background app not running
    status: Option<Status>,
    /// bottom message: (text, is_error)
    message: Option<(SharedString, bool)>,
    _subs: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg = Config::load();
        let mut subs = Vec::new();

        // ---- X / Y / W / H / screen size: number inputs with +/- steppers
        let mut nums = Vec::new();
        for i in 0..NUM_LABELS.len() {
            let v = get_num(&cfg, i);
            let state = cx.new(|cx| InputState::new(window, cx).default_value(v.to_string()));

            // typed values
            subs.push(cx.subscribe_in(
                &state,
                window,
                move |this: &mut Self, state, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Change { .. } => {
                        let text = state.read(cx).value().to_string();
                        if let Ok(v) = text.trim().parse::<u32>() {
                            this.set_num(i, v, cx);
                        }
                    }
                    // show the clamped value once the user leaves the field
                    InputEvent::Blur { .. } => this.sync_input(i, window, cx),
                    _ => {}
                },
            ));
            // +/- buttons and arrow keys (Shift = 10 px)
            subs.push(cx.subscribe_in(
                &state,
                window,
                move |this: &mut Self, state, ev: &NumberInputEvent, window, cx| {
                    let NumberInputEvent::Step(action) = ev;
                    {
                        let step = if window.modifiers().shift { 10 } else { 1 };
                        let cur = get_num(&this.cfg, i);
                        let new = match action {
                            StepAction::Increment => cur.saturating_add(step),
                            StepAction::Decrement => cur.saturating_sub(step),
                        };
                        this.set_num(i, new, cx);
                        let v = get_num(&this.cfg, i).to_string();
                        state.update(cx, |s, cx| s.set_value(v, window, cx));
                    }
                },
            ));
            nums.push(state);
        }

        // ---- file name prefix
        let prefix = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("bandicam")
                .default_value(cfg.prefix.clone())
        });
        subs.push(cx.subscribe_in(
            &prefix,
            window,
            |this: &mut Self, state, ev: &InputEvent, _, cx| {
                if let InputEvent::Change { .. } = ev {
                    let v = state.read(cx).value().trim().to_string();
                    if !v.is_empty() && v != this.cfg.prefix {
                        this.cfg.prefix = v;
                        this.commit(cx);
                    }
                }
            },
        ));

        // ---- JPEG quality
        let quality = cx.new(|_| {
            SliderState::new()
                .min(1.)
                .max(100.)
                .step(1.)
                .default_value(cfg.jpeg_quality as f32)
        });
        subs.push(cx.subscribe_in(
            &quality,
            window,
            |this: &mut Self, _, ev: &SliderEvent, _, cx| {
                #[allow(irrefutable_let_patterns)]
                let SliderEvent::Change(v) = ev else { return };
                let q = match v {
                    SliderValue::Single(v) => *v,
                    SliderValue::Range(_, v) => *v,
                }
                .round()
                .clamp(1., 100.) as u8;
                if q != this.cfg.jpeg_quality {
                    this.cfg.jpeg_quality = q;
                    this.commit(cx);
                }
            },
        ));

        // ---- keep in sync with the background app (frame dragged on
        //      screen, tray toggles, new captures)
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                let st = cx
                    .background_executor()
                    .spawn(async { bcshot_common::status().ok() })
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.poll(st, window, cx))
                    .is_err()
                {
                    break; // window closed
                }
            }
        })
        .detach();

        Self {
            cfg_mtime: Config::mtime(),
            status: bcshot_common::status().ok(),
            cfg,
            nums,
            prefix,
            quality,
            message: None,
            _subs: subs,
        }
    }

    // ------------------------------------------------------------ state

    fn set_num(&mut self, i: usize, v: u32, cx: &mut Context<Self>) {
        let mut c = self.cfg.clone();
        match i {
            0 => c.x = v,
            1 => c.y = v,
            2 => c.width = v,
            3 => c.height = v,
            4 => c.ref_w = v.clamp(320, 16384),
            _ => c.ref_h = v.clamp(200, 16384),
        }
        // keep the rectangle on screen
        c.width = c.width.clamp(MIN_PX, c.ref_w);
        c.height = c.height.clamp(MIN_PX, c.ref_h);
        c.x = c.x.min(c.ref_w - c.width);
        c.y = c.y.min(c.ref_h - c.height);
        if c != self.cfg {
            self.cfg = c;
            self.commit(cx);
        }
    }

    fn sync_input(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let v = get_num(&self.cfg, i).to_string();
        let state = self.nums[i].clone();
        if state.read(cx).value().to_string() != v {
            state.update(cx, |s, cx| s.set_value(v, window, cx));
        }
    }

    /// Save, and tell the background app to reload.
    fn commit(&mut self, cx: &mut Context<Self>) {
        match self.cfg.save() {
            Ok(()) => self.cfg_mtime = Config::mtime(),
            Err(e) => self.message = Some((format!("Couldn't save settings: {e}").into(), true)),
        }
        cx.background_executor()
            .spawn(async {
                let _ = bcshot_common::send("reload");
            })
            .detach();
        cx.notify();
    }

    fn poll(&mut self, st: Option<Status>, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = st != self.status;
        self.status = st;

        let mt = Config::mtime();
        if mt != self.cfg_mtime {
            self.cfg_mtime = mt;
            let new = Config::load();
            if new != self.cfg {
                self.cfg = new;
                for i in 0..self.nums.len() {
                    self.sync_input(i, window, cx);
                }
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    // ------------------------------------------------------------ actions

    fn capture(&mut self, cx: &mut Context<Self>) {
        self.message = Some(("Capturing…".into(), false));
        let task = cx
            .background_executor()
            .spawn(async { bcshot_common::send("capture") });
        cx.spawn(async move |this, cx| {
            let r = task.await;
            this.update(cx, |this, cx| {
                this.message = Some(match r {
                    Ok(s) if s.starts_with("OK ") => {
                        let name = PathBuf::from(&s[3..])
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        (format!("Saved {name}").into(), false)
                    }
                    Ok(s) => (s.trim_start_matches("ERR ").to_string().into(), true),
                    Err(_) => ("The background app isn't running.".into(), true),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle_adjust(&mut self, cx: &mut Context<Self>) {
        cx.background_executor()
            .spawn(async {
                let _ = bcshot_common::send("edit");
            })
            .detach();
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        let start = self.cfg.out_dir.clone();
        cx.spawn(async move |this, cx| {
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Choose the screenshots folder")
                .set_directory(&start)
                .pick_folder()
                .await;
            if let Some(dir) = picked {
                this.update(cx, |this, cx| {
                    this.cfg.out_dir = dir.path().to_path_buf();
                    this.commit(cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn start_background_app(&mut self, cx: &mut Context<Self>) {
        let _ = Command::new(bcshot_common::sibling_exe("bcshot"))
            .env("BCSHOT_FROM_SETTINGS", "1")
            .spawn();
        self.message = Some(("Starting the background app…".into(), false));
        cx.notify();
    }

    // ------------------------------------------------------------ pieces

    fn card(title: &str, subtitle: Option<String>, cx: &App) -> Div {
        let t = cx.theme();
        v_flex()
            .gap_3()
            .p_4()
            .rounded_xl()
            .border_1()
            .border_color(t.border)
            .bg(t.secondary.opacity(0.35))
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
                    .children(
                        subtitle.map(|s| div().text_xs().text_color(t.muted_foreground).child(s)),
                    ),
            )
    }

    fn field(label: &str, el: impl IntoElement, cx: &App) -> Div {
        v_flex()
            .gap_1()
            .flex_1()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.to_string()),
            )
            .child(el)
    }

    fn row(title: &str, hint: &str, el: impl IntoElement, cx: &App) -> Div {
        h_flex()
            .justify_between()
            .items_center()
            .gap_4()
            .child(
                v_flex()
                    .child(div().text_sm().child(title.to_string()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(hint.to_string()),
                    ),
            )
            .child(el)
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let running = self.status.is_some();
        let editing = self.status.as_ref().map(|s| s.editing).unwrap_or(false);
        let (pill, color) = match (running, editing) {
            (false, _) => ("Not running", t.danger),
            (true, true) => ("Adjusting frame", t.warning),
            (true, false) => ("Frame locked", t.success),
        };

        h_flex()
            .justify_between()
            .items_center()
            .child(
                v_flex()
                    .child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).child("bcshot"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(t.muted_foreground)
                            .child("Rectangle screenshots, Bandicam style"),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(color.opacity(0.15))
                            .text_color(color)
                            .text_xs()
                            .child(div().size_2().rounded_full().bg(color))
                            .child(pill),
                    )
                    .child(
                        Button::new("adjust")
                            .label(if editing {
                                "Lock frame"
                            } else {
                                "Adjust frame"
                            })
                            .outline()
                            .disabled(!running)
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_adjust(cx)),
                            ),
                    )
                    .child(
                        Button::new("capture")
                            .label("Capture")
                            .primary()
                            .disabled(!running || editing)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.capture(cx))),
                    ),
            )
    }

    fn area_card(&self, cx: &mut Context<Self>) -> Div {
        let c = &self.cfg;
        Self::card(
            "Capture area",
            Some(format!("{} × {} px", c.width, c.height)),
            cx,
        )
        .child(
            h_flex()
                .gap_3()
                .child(Self::field(
                    "X",
                    NumberInput::new(&self.nums[0]).small(),
                    cx,
                ))
                .child(Self::field(
                    "Y",
                    NumberInput::new(&self.nums[1]).small(),
                    cx,
                ))
                .child(Self::field(
                    "Width",
                    NumberInput::new(&self.nums[2]).small(),
                    cx,
                ))
                .child(Self::field(
                    "Height",
                    NumberInput::new(&self.nums[3]).small(),
                    cx,
                )),
        )
        .child(Self::row(
            "Show frame while locked",
            "Blue border around the area; clicks go through it",
            Switch::new("show-frame")
                .checked(c.show_frame)
                .on_click(cx.listener(|this, on: &bool, _, cx| {
                    this.cfg.show_frame = *on;
                    this.commit(cx);
                })),
            cx,
        ))
        .child(
            h_flex()
                .gap_3()
                .items_end()
                .child(Self::field(
                    "Screen width",
                    NumberInput::new(&self.nums[4]).small(),
                    cx,
                ))
                .child(Self::field(
                    "Screen height",
                    NumberInput::new(&self.nums[5]).small(),
                    cx,
                ))
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            "Your native resolution. Scaling (125 %, 150 %) is handled for you.",
                        ),
                ),
        )
    }

    fn output_card(&self, cx: &mut Context<Self>) -> Div {
        let c = &self.cfg;
        let t = cx.theme();
        let jpg = c.format == ImageFormat::Jpg;
        let ext = if jpg { "jpg" } else { "png" };
        let fmt_btn = |id: &'static str, label: &'static str, sel: bool, f: ImageFormat| {
            let b = Button::new(id).label(label).small();
            let b = if sel { b.primary() } else { b.ghost() };
            b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.cfg.format = f;
                this.commit(cx);
            }))
        };

        Self::card("Output", None, cx)
            .child(Self::field(
                "Folder",
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .px_3()
                            .py_1p5()
                            .rounded_md()
                            .border_1()
                            .border_color(t.border)
                            .bg(t.background)
                            .text_sm()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(c.out_dir.display().to_string()),
                    )
                    .child(
                        Button::new("browse")
                            .label("Browse…")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.browse(cx))),
                    )
                    .child(
                        Button::new("open-dir")
                            .label("Open")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, _| {
                                let _ = std::fs::create_dir_all(&this.cfg.out_dir);
                                let _ = Command::new("xdg-open").arg(&this.cfg.out_dir).spawn();
                            })),
                    ),
                cx,
            ))
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .child(Self::field(
                        "File name prefix",
                        Input::new(&self.prefix).small(),
                        cx,
                    ))
                    .child(Self::field(
                        "Format",
                        h_flex()
                            .gap_1()
                            .p_0p5()
                            .rounded_md()
                            .border_1()
                            .border_color(t.border)
                            .child(fmt_btn("fmt-jpg", "JPG", jpg, ImageFormat::Jpg))
                            .child(fmt_btn("fmt-png", "PNG", !jpg, ImageFormat::Png)),
                        cx,
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(t.muted_foreground)
                    .child(format!("e.g. {} 2026-10-28 20-08-18-261.{ext}", c.prefix)),
            )
            .when(jpg, |d| {
                d.child(Self::field(
                    &format!("JPEG quality: {}", c.jpeg_quality),
                    Slider::new(&self.quality),
                    cx,
                ))
            })
            .child(Self::row(
                "Shutter sound",
                "Play a camera sound on capture",
                Switch::new("sound")
                    .checked(c.shutter_sound)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.cfg.shutter_sound = *on;
                        this.commit(cx);
                    })),
                cx,
            ))
            .child(Self::row(
                "Notification",
                "Thumbnail with Open / Show in folder",
                Switch::new("notify")
                    .checked(c.notify)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.cfg.notify = *on;
                        this.commit(cx);
                    })),
                cx,
            ))
    }

    fn hotkeys_card(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let line = |id: &'static str, cmd: &'static str, what: &'static str| {
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(t.background)
                        .border_1()
                        .border_color(t.border)
                        .font_family("monospace")
                        .text_sm()
                        .child(cmd),
                )
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(t.muted_foreground)
                        .child(what),
                )
                .child(Button::new(id).label("Copy").small().ghost().on_click(
                    move |_: &ClickEvent, _, cx: &mut App| {
                        cx.write_to_clipboard(ClipboardItem::new_string(cmd.to_string()))
                    },
                ))
        };
        Self::card("Hotkeys", None, cx)
            .child(line("copy-cap", "bcshot capture", "Take a screenshot"))
            .child(line("copy-edit", "bcshot edit", "Adjust / lock the frame"))
            .child(div().text_xs().text_color(t.muted_foreground).child(
                "KDE: System Settings → Keyboard → Shortcuts → Add New → Command or Script. \
                         GNOME: Settings → Keyboard → Custom Shortcuts. No plug-in needed.",
            ))
    }

    fn preview_card(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme();
        let last = self
            .status
            .as_ref()
            .and_then(|s| s.last_capture.clone())
            .filter(|p| p.exists());

        let card = Self::card("Last capture", None, cx).flex_1();
        match last {
            None => card.child(
                v_flex()
                    .flex_1()
                    .min_h(px(320.))
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(t.border)
                    .child(div().text_sm().child("Nothing captured yet"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(t.muted_foreground)
                            .child("Press your capture hotkey, or the Capture button"),
                    ),
            ),
            Some(p) => {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let (p1, p2) = (p.clone(), p.clone());
                card.child(
                    div()
                        .flex_1()
                        .min_h(px(320.))
                        .rounded_lg()
                        .bg(t.background)
                        .p_2()
                        .child(img(p).size_full().object_fit(ObjectFit::Contain)),
                )
                .child(div().text_xs().text_color(t.muted_foreground).child(name))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("open-last")
                                .label("Open")
                                .small()
                                .outline()
                                .on_click(move |_: &ClickEvent, _, _: &mut App| {
                                    let _ = Command::new("xdg-open").arg(&p1).spawn();
                                }),
                        )
                        .child(
                            Button::new("show-last")
                                .label("Show in folder")
                                .small()
                                .ghost()
                                .on_click(move |_: &ClickEvent, _, _: &mut App| {
                                    show_in_folder(&p2)
                                }),
                        ),
                )
            }
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.status.is_some();
        let header = self.header(cx);
        let area = self.area_card(cx);
        let output = self.output_card(cx);
        let hotkeys = self.hotkeys_card(cx);
        let preview = self.preview_card(cx);
        let t = cx.theme();

        v_flex()
            .size_full()
            .bg(t.background)
            .text_color(t.foreground)
            .p_5()
            .gap_4()
            .child(header)
            .when(!running, |d| {
                d.child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .p_3()
                        .rounded_lg()
                        .bg(t.danger.opacity(0.12))
                        .border_1()
                        .border_color(t.danger.opacity(0.4))
                        .child(div().text_sm().child(
                            "The background app isn't running, so the frame and hotkeys are off.",
                        ))
                        .child(
                            Button::new("start")
                                .label("Start it")
                                .small()
                                .primary()
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.start_background_app(cx)
                                })),
                        ),
                )
            })
            .child(
                h_flex()
                    .flex_1()
                    .gap_4()
                    .items_start()
                    .child(
                        v_flex()
                            .w(px(540.))
                            .gap_4()
                            .child(area)
                            .child(output)
                            .child(hotkeys),
                    )
                    .child(v_flex().flex_1().h_full().child(preview)),
            )
            .children(self.message.as_ref().map(|(text, err)| {
                div()
                    .text_sm()
                    .text_color(if *err { t.danger } else { t.muted_foreground })
                    .child(text.clone())
            }))
    }
}

fn get_num(c: &Config, i: usize) -> u32 {
    match i {
        0 => c.x,
        1 => c.y,
        2 => c.width,
        3 => c.height,
        4 => c.ref_w,
        _ => c.ref_h,
    }
}

/// Dolphin / Nautilus with the file selected, else just the folder.
fn show_in_folder(path: &PathBuf) {
    let uri = format!("file://{}", path.display());
    let ok = Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("array:string:{uri}"),
            "string:",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        if let Some(dir) = path.parent() {
            let _ = Command::new("xdg-open").arg(dir).spawn();
        }
    }
}
