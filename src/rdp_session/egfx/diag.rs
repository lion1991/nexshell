//! EGFX 运行时诊断（NEXSHELL_RDP_EGFX_DIAG）：每 ~3s 打一行各操作**窗口增量**计数，
//! 真机一行即可看清服务端发了什么 codec 组合、用了多少合成/cache 操作、我们丢了什么。
//! 丢弃类（未解码的 wire1 codec、progressive 解码失败）单列，非 0 时 `DROP=` 段高亮。

use std::time::{Duration, Instant};

use ironrdp_egfx::pdu::Codec1Type;

/// 一个刷新窗口内的各操作计数（打印后清零）。
#[derive(Default)]
struct Counts {
    // wire1（已由库解码 → on_bitmap_updated）
    avc420: u64,
    clearcodec: u64,
    uncompressed: u64,
    // wire1（库未解码 → on_unhandled_pdu，=丢弃）
    avc444: u64,
    other_wire1: u64,
    // wire2
    progressive: u64,
    prog_fail: u64,  // 真正解码 Err（计入 DROP）
    prog_empty: u64, // 0-tile 正常 PDU（SYNC/帧标记，不计 DROP）
    // 合成 / cache
    solid_fill: u64,
    s2s: u64,
    s2c: u64,
    c2s: u64,
    evict: u64,
    // 生命周期
    surf_create: u64,
    surf_delete: u64,
    map: u64,
    map_scaled: u64,
    end_frame: u64, // = 已发 FrameAcknowledge 数（库在 EndFrame 处 1:1 发 ack）
    pipeline_error: u64,
}

/// EGFX 诊断累加器：禁用时全部方法零成本（只判 `enabled`）。
pub struct EgfxDiag {
    enabled: bool,
    last_flush: Instant,
    cur: Counts,
    total_end_frames: u64,
}

impl EgfxDiag {
    pub fn new() -> Self {
        Self {
            enabled: std::env::var_os("NEXSHELL_RDP_EGFX_DIAG").is_some(),
            last_flush: Instant::now(),
            cur: Counts::default(),
            total_end_frames: 0,
        }
    }

    pub fn on_bitmap(&mut self, codec: Codec1Type) {
        if !self.enabled {
            return;
        }
        match codec {
            Codec1Type::Avc420 => self.cur.avc420 += 1,
            Codec1Type::ClearCodec => self.cur.clearcodec += 1,
            Codec1Type::Uncompressed => self.cur.uncompressed += 1,
            _ => self.cur.other_wire1 += 1,
        }
    }

    /// 库未内部解码的 wire1（丢弃）。
    pub fn on_unhandled_wire1(&mut self, codec: Codec1Type) {
        if !self.enabled {
            return;
        }
        match codec {
            Codec1Type::Avc444 | Codec1Type::Avc444v2 => self.cur.avc444 += 1,
            _ => self.cur.other_wire1 += 1,
        }
    }

    pub fn on_progressive(&mut self, tiles: usize, failed: bool) {
        if !self.enabled {
            return;
        }
        self.cur.progressive += 1;
        if failed {
            self.cur.prog_fail += 1;
        } else if tiles == 0 {
            self.cur.prog_empty += 1;
        }
    }

    pub fn on_solid_fill(&mut self) {
        self.cur.solid_fill += self.enabled as u64;
    }
    pub fn on_s2s(&mut self) {
        self.cur.s2s += self.enabled as u64;
    }
    pub fn on_s2c(&mut self) {
        self.cur.s2c += self.enabled as u64;
    }
    pub fn on_c2s(&mut self) {
        self.cur.c2s += self.enabled as u64;
    }
    pub fn on_evict(&mut self) {
        self.cur.evict += self.enabled as u64;
    }
    pub fn on_surf_create(&mut self) {
        self.cur.surf_create += self.enabled as u64;
    }
    pub fn on_surf_delete(&mut self) {
        self.cur.surf_delete += self.enabled as u64;
    }
    pub fn on_map(&mut self, scaled: bool) {
        if !self.enabled {
            return;
        }
        if scaled {
            self.cur.map_scaled += 1;
        } else {
            self.cur.map += 1;
        }
    }
    pub fn on_pipeline_error(&mut self) {
        self.cur.pipeline_error += self.enabled as u64;
    }

    /// 每帧末调用（on_frame_complete）：累加 end_frame，并按 3s 节流打印窗口增量。
    pub fn on_end_frame(&mut self) {
        if !self.enabled {
            return;
        }
        self.cur.end_frame += 1;
        self.total_end_frames += 1;
        if self.last_flush.elapsed() >= Duration::from_secs(3) {
            self.flush();
        }
    }

    fn flush(&mut self) {
        let c = &self.cur;
        let drop_total = c.avc444 + c.other_wire1 + c.prog_fail;
        let drop = if drop_total > 0 {
            format!(
                " DROP={{avc444={} other_wire1={} prog_fail={}}}",
                c.avc444, c.other_wire1, c.prog_fail
            )
        } else {
            String::new()
        };
        eprintln!(
            "[egfx-diag] {dt:.1}s wire1{{avc420={} clear={} uncomp={}}} \
prog={}(empty={}) fill={} s2s={} s2c={} c2s={} evict={} \
surf{{+{} -{}}} map={}(scaled={}) endframe/ack={}(tot={}) err={}{drop}",
            c.avc420,
            c.clearcodec,
            c.uncompressed,
            c.progressive,
            c.prog_empty,
            c.solid_fill,
            c.s2s,
            c.s2c,
            c.c2s,
            c.evict,
            c.surf_create,
            c.surf_delete,
            c.map,
            c.map_scaled,
            c.end_frame,
            self.total_end_frames,
            c.pipeline_error,
            dt = self.last_flush.elapsed().as_secs_f64(),
        );
        self.cur = Counts::default();
        self.last_flush = Instant::now();
    }
}
