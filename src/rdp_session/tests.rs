use super::*;

#[test]
fn domain_user_splits_on_backslash() {
    assert_eq!(
        split_domain_user("CORP\\alice"),
        (Some("CORP".to_string()), "alice".to_string())
    );
}

#[test]
fn plain_user_has_no_domain() {
    assert_eq!(split_domain_user("bob"), (None, "bob".to_string()));
}

#[test]
fn empty_domain_falls_back_to_local() {
    assert_eq!(split_domain_user("\\svc"), (None, "\\svc".to_string()));
}

#[test]
fn egfx_defaults_on_when_not_disabled() {
    assert!(default_enable_egfx_from_env(None));
}

#[test]
fn egfx_can_be_disabled_for_legacy_fallback() {
    assert!(!default_enable_egfx_from_env(Some(
        std::ffi::OsString::from("1")
    )));
}

#[test]
fn audio_static_channels_include_rdpdr_dependency() {
    // 重构后 rdpsnd 与 rdpdr 分别注册：照 connect_and_run 的组装方式验证「audio 开时 rdpdr 在场」。
    let config = RdpSessionConfig {
        host: "127.0.0.1".to_string(),
        port: 3389,
        username: "alice".to_string(),
        password: "secret".to_string(),
        width: 1024,
        height: 768,
        enable_egfx: false,
        enable_audio: true,
        enable_drive: false,
        desktop_scale_factor: 100,
        enable_udp: false,
    };
    let connector = ClientConnector::new(
        build_connector_config(&config),
        "127.0.0.1:0".parse().expect("valid loopback socket addr"),
    );
    let mut connector = attach_audio_static_channels(
        connector,
        ironrdp_rdpsnd::client::Rdpsnd::new(Box::new(ironrdp_rdpsnd::client::NoopRdpsndBackend)),
    );
    if let Some(channel) = rdpdr::build_channel(config.enable_drive, config.enable_audio) {
        connector = connector.with_static_channel(channel);
    }

    let names = audio_diag::static_channel_names(&connector.static_channels);

    assert!(names.iter().any(|name| name == "rdpsnd"));
    assert!(
        names.iter().any(|name| name == "rdpdr"),
        "FreeRDP enables rdpdr when rdpsnd is present; got {names:?}"
    );
}

#[test]
fn connection_type_is_lan_unless_autodetect_requested() {
    use ironrdp_pdu::gcc::ConnectionType;
    assert_eq!(connection_type_from_env(None), ConnectionType::Lan);
    assert_eq!(
        connection_type_from_env(Some("1".into())),
        ConnectionType::Autodetect
    );
}

#[test]
fn connector_config_follows_udp_gate_and_requires_egfx() {
    let mut config = RdpSessionConfig {
        host: "127.0.0.1".to_string(),
        port: 3389,
        username: "alice".to_string(),
        password: "secret".to_string(),
        width: 1024,
        height: 768,
        enable_egfx: true,
        enable_audio: false,
        enable_drive: false,
        desktop_scale_factor: 100,
        enable_udp: true,
    };
    let built = build_connector_config(&config);
    assert_eq!(
        built.connection_type,
        connection_type_from_env(std::env::var_os("NEXSHELL_RDP_AUTODETECT"))
    );
    let flags = built
        .multitransport_flags
        .expect("UDP on advertises multitransport");
    assert!(flags.contains(ironrdp_pdu::gcc::MultiTransportFlags::SOFT_SYNC_TCP_TO_UDP));

    // 关 EGFX 就没有 DRDYNVC，不能声明 UDP。
    config.enable_egfx = false;
    assert!(build_connector_config(&config)
        .multitransport_flags
        .is_none());

    config.enable_egfx = true;
    config.enable_udp = false;
    assert!(build_connector_config(&config)
        .multitransport_flags
        .is_none());
}

#[test]
fn rdpsnd_companion_follows_audio_or_drive() {
    // 核心修复：drive 单开（audio 关）也必须挂 rdpsnd 伴随通道。
    assert!(needs_rdpsnd(true, false));
    assert!(needs_rdpsnd(false, true));
    assert!(needs_rdpsnd(true, true));
    assert!(!needs_rdpsnd(false, false));
}

#[test]
fn drive_only_advertises_rdpsnd_and_rdpdr() {
    // audio 关、drive 开：照 connect_and_run 组装，验证 rdpsnd 伴随 + rdpdr 同时在场。
    let config = RdpSessionConfig {
        host: "127.0.0.1".to_string(),
        port: 3389,
        username: "alice".to_string(),
        password: "secret".to_string(),
        width: 1024,
        height: 768,
        enable_egfx: false,
        enable_audio: false,
        enable_drive: true,
        desktop_scale_factor: 100,
        enable_udp: false,
    };
    let mut connector = ClientConnector::new(
        build_connector_config(&config),
        "127.0.0.1:0".parse().expect("valid loopback socket addr"),
    );
    // drive 开、audio 关 → Noop rdpsnd 静默伴随（单测里避免真实 cpal 设备）。
    if needs_rdpsnd(config.enable_audio, config.enable_drive) {
        connector = attach_audio_static_channels(
            connector,
            ironrdp_rdpsnd::client::Rdpsnd::new(Box::new(
                ironrdp_rdpsnd::client::NoopRdpsndBackend,
            )),
        );
    }
    if let Some(channel) = rdpdr::build_channel(config.enable_drive, config.enable_audio) {
        connector = connector.with_static_channel(channel);
    }

    let names = audio_diag::static_channel_names(&connector.static_channels);
    assert!(
        names.iter().any(|name| name == "rdpsnd"),
        "drive needs rdpsnd companion (MS-RDPEFS); got {names:?}"
    );
    assert!(
        names.iter().any(|name| name == "rdpdr"),
        "drive registers rdpdr; got {names:?}"
    );
}

#[test]
fn apply_full_copies_whole_frame() {
    let mut fb = RdpFramebuffer::new(2, 2);
    let src = vec![9u8; 2 * 2 * 4];
    let dirty = fb.apply_full(&src);
    assert_eq!(
        dirty,
        DirtyRect {
            x: 0,
            y: 0,
            width: 2,
            height: 2
        }
    );
    assert!(fb.rgba.iter().all(|&b| b == 9));
}

#[test]
fn generation_advances_on_each_apply() {
    let mut fb = RdpFramebuffer::new(2, 2);
    assert_eq!(fb.generation(), 0);
    fb.apply_full(&vec![1u8; 2 * 2 * 4]);
    assert_eq!(fb.generation(), 1);
    fb.apply_region(
        &vec![2u8; 2 * 2 * 4],
        DirtyRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    assert_eq!(fb.generation(), 2);
}

#[test]
fn key_press_and_release_flags() {
    use ironrdp_pdu::input::fast_path::KeyboardFlags;
    match to_fastpath_input(RdpInputEvent::Key {
        scancode: 0x1E,
        extended: false,
        pressed: true,
    }) {
        FastPathInputEvent::KeyboardEvent(flags, code) => {
            assert_eq!(code, 0x1E);
            assert!(!flags.contains(KeyboardFlags::RELEASE));
            assert!(!flags.contains(KeyboardFlags::EXTENDED));
        }
        other => panic!("expected KeyboardEvent, got {other:?}"),
    }
    match to_fastpath_input(RdpInputEvent::Key {
        scancode: 0x48,
        extended: true,
        pressed: false,
    }) {
        FastPathInputEvent::KeyboardEvent(flags, _) => {
            assert!(flags.contains(KeyboardFlags::RELEASE));
            assert!(flags.contains(KeyboardFlags::EXTENDED));
        }
        other => panic!("expected KeyboardEvent, got {other:?}"),
    }
}

#[test]
fn mouse_move_and_button_flags() {
    use ironrdp_pdu::input::mouse::PointerFlags;
    match to_fastpath_input(RdpInputEvent::MouseMove { x: 10, y: 20 }) {
        FastPathInputEvent::MouseEvent(pdu) => {
            assert!(pdu.flags.contains(PointerFlags::MOVE));
            assert_eq!((pdu.x_position, pdu.y_position), (10, 20));
        }
        other => panic!("expected MouseEvent, got {other:?}"),
    }
    match to_fastpath_input(RdpInputEvent::MouseButton {
        button: RdpButton::Left,
        pressed: true,
        x: 1,
        y: 2,
    }) {
        FastPathInputEvent::MouseEvent(pdu) => {
            assert!(pdu.flags.contains(PointerFlags::LEFT_BUTTON));
            assert!(pdu.flags.contains(PointerFlags::DOWN));
        }
        other => panic!("expected MouseEvent, got {other:?}"),
    }
    // 右键抬起：RIGHT_BUTTON 且无 DOWN。
    match to_fastpath_input(RdpInputEvent::MouseButton {
        button: RdpButton::Right,
        pressed: false,
        x: 0,
        y: 0,
    }) {
        FastPathInputEvent::MouseEvent(pdu) => {
            assert!(pdu.flags.contains(PointerFlags::RIGHT_BUTTON));
            assert!(!pdu.flags.contains(PointerFlags::DOWN));
        }
        other => panic!("expected MouseEvent, got {other:?}"),
    }
}

#[test]
fn wheel_direction_and_sign() {
    use ironrdp_pdu::input::mouse::PointerFlags;
    match to_fastpath_input(RdpInputEvent::Wheel {
        horizontal: false,
        delta: -120,
        x: 5,
        y: 6,
    }) {
        FastPathInputEvent::MouseEvent(pdu) => {
            assert!(pdu.flags.contains(PointerFlags::VERTICAL_WHEEL));
            assert_eq!(pdu.number_of_wheel_rotation_units, -120);
        }
        other => panic!("expected MouseEvent, got {other:?}"),
    }
    match to_fastpath_input(RdpInputEvent::Wheel {
        horizontal: true,
        delta: 120,
        x: 0,
        y: 0,
    }) {
        FastPathInputEvent::MouseEvent(pdu) => {
            assert!(pdu.flags.contains(PointerFlags::HORIZONTAL_WHEEL));
        }
        other => panic!("expected MouseEvent, got {other:?}"),
    }
}

fn incl(left: u16, top: u16, right: u16, bottom: u16) -> InclusiveRectangle {
    InclusiveRectangle {
        left,
        top,
        right,
        bottom,
    }
}

#[test]
fn inclusive_to_dirty_uses_inclusive_bounds() {
    // 0..=1 两轴 → 2x2 起点(0,0)。
    assert_eq!(
        inclusive_to_dirty(&incl(0, 0, 1, 1), 100, 100),
        DirtyRect {
            x: 0,
            y: 0,
            width: 2,
            height: 2
        }
    );
    // 单像素 rect：右=左、下=上 → 1x1。
    assert_eq!(
        inclusive_to_dirty(&incl(5, 7, 5, 7), 100, 100),
        DirtyRect {
            x: 5,
            y: 7,
            width: 1,
            height: 1
        }
    );
}

#[test]
fn inclusive_to_dirty_clamps_to_desktop() {
    // 越界的 right/bottom 收敛到 max-1，宽高不超出画面。
    let d = inclusive_to_dirty(&incl(3, 3, 99, 99), 10, 10);
    assert_eq!(d.x, 3);
    assert_eq!(d.y, 3);
    assert_eq!(d.x + d.width, 10);
    assert_eq!(d.y + d.height, 10);
    // 零尺寸桌面不 panic，返回空矩形。
    assert_eq!(inclusive_to_dirty(&incl(0, 0, 1, 1), 0, 0).width, 0);
}

#[test]
fn union_dirty_bounding_box() {
    let a = DirtyRect {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
    };
    let b = DirtyRect {
        x: 5,
        y: 6,
        width: 3,
        height: 4,
    };
    assert_eq!(
        union_dirty(a, b),
        DirtyRect {
            x: 0,
            y: 0,
            width: 8,
            height: 10
        }
    );
    // 自并保持不变。
    assert_eq!(union_dirty(a, a), a);
}

#[test]
fn apply_region_copies_only_target_rows() {
    // 4x4 全 0，src 全 7，只覆盖右下角 2x2。
    let mut fb = RdpFramebuffer::new(4, 4);
    let src = vec![7u8; 4 * 4 * 4];
    fb.apply_region(
        &src,
        DirtyRect {
            x: 2,
            y: 2,
            width: 2,
            height: 2,
        },
    );
    let stride = 4 * 4;
    // 顶部两行仍全 0。
    assert!(fb.rgba[..2 * stride].iter().all(|&b| b == 0));
    // 第 3 行前 2 像素(0..8) 仍 0，后 2 像素(8..16) 被覆盖成 7。
    let row2 = &fb.rgba[2 * stride..3 * stride];
    assert!(row2[..8].iter().all(|&b| b == 0));
    assert!(row2[8..16].iter().all(|&b| b == 7));
}

fn make_pointer(px: u8) -> Arc<ironrdp_graphics::pointer::DecodedPointer> {
    Arc::new(ironrdp_graphics::pointer::DecodedPointer {
        width: 2,
        height: 2,
        hotspot_x: 1,
        hotspot_y: 0,
        bitmap_data: vec![px; 2 * 2 * 4],
    })
}

#[test]
fn pointer_bitmap_maps_fields() {
    let mut last = None;
    let p = make_pointer(9);
    let event = pointer_to_event(&p, &mut last).expect("first pointer emitted");
    match event {
        RdpPointer::Bitmap {
            rgba,
            width,
            height,
            hotspot_x,
            hotspot_y,
            cache_key,
        } => {
            assert_eq!(rgba, vec![9u8; 16]);
            assert_eq!((width, height), (2, 2));
            assert_eq!((hotspot_x, hotspot_y), (1.0, 0.0));
            assert_eq!(last, Some(cache_key));
            // 内容 hash：不同 Arc、同内容 → 同 key（地址语义做不到）。
            let mut last2 = None;
            match pointer_to_event(&make_pointer(9), &mut last2).expect("second pointer emitted") {
                RdpPointer::Bitmap { cache_key: k2, .. } => assert_eq!(cache_key, k2),
                other => panic!("expected Bitmap, got {other:?}"),
            }
        }
        other => panic!("expected Bitmap, got {other:?}"),
    }
}

#[test]
fn pointer_same_arc_dedups() {
    let mut last = None;
    let p = make_pointer(1);
    assert!(pointer_to_event(&p, &mut last).is_some());
    // 同一 Arc 再来一次 → 去重返回 None。
    assert!(pointer_to_event(&p, &mut last).is_none());
}

#[test]
fn pointer_different_arc_reemits() {
    let mut last = None;
    let a = make_pointer(1);
    let b = make_pointer(2);
    assert!(pointer_to_event(&a, &mut last).is_some());
    // 内容不同 → 重新发送。
    assert!(pointer_to_event(&b, &mut last).is_some());
}

#[test]
fn pointer_same_content_different_arc_dedups() {
    let mut last = None;
    assert!(pointer_to_event(&make_pointer(1), &mut last).is_some());
    // 新 Arc 但内容相同 → 内容 hash 去重返回 None（地址语义会误重发）。
    assert!(pointer_to_event(&make_pointer(1), &mut last).is_none());
}
