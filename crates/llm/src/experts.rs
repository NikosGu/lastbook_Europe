//! A mixture of experts read from the disk, faster. Each word uses a few experts per layer (8 of 256 in Qwen3.6-35B-A3B),
//! chosen by the layer's router just before they are used. Left to themselves, the chosen experts' weights come off the
//! disk in small pieces as the computation touches them. Here llama.cpp's evaluation callback shows the runner each
//! layer's choice (the "ffn_moe_topk-<layer>" node) as soon as it is computed, and the runner asks the system for all of
//! the chosen experts' weights at once, so they are read together, in parallel, while the computation reaches them.
//! The weights stay where llama.cpp put them, in the mapped model file: nothing is copied and the answer is the same.
//!
//! The idea, and the node names to watch, come from BigMoeOnEdge (github.com/Helldez/BigMoeOnEdge), which goes further
//! (a cache of its own, kept by use).

use std::collections::HashMap;
use std::ffi::{c_void, CStr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Where one layer's experts lie in memory: for each weight tensor (gate, up, down, or gate and up together), the address
/// of expert 0, the bytes from one expert to the next, and the number of experts.
#[derive(Default)]
struct Layer {
    tensors: Vec<(usize, usize, usize)>,
}

pub struct Prefetch {
    layers: Mutex<HashMap<u32, Layer>>,
    page: usize,
    /// How many expert slices were asked for, for the log.
    pub asked: AtomicU64,
}

impl Prefetch {
    pub fn new() -> Prefetch {
        #[cfg(unix)]
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as usize;
        #[cfg(not(unix))]
        let page = 4096;
        Prefetch { layers: Mutex::new(HashMap::new()), page, asked: AtomicU64::new(0) }
    }

    fn record(&self, layer: u32, data: usize, stride: usize, n: usize) {
        let mut layers = self.layers.lock().unwrap_or_else(|e| e.into_inner());
        let l = layers.entry(layer).or_default();
        if !l.tensors.iter().any(|t| t.0 == data) {
            l.tensors.push((data, stride, n));
        }
    }

    /// Ask the system for the chosen experts of one layer.
    fn want(&self, layer: u32, experts: &[usize]) {
        let layers = self.layers.lock().unwrap_or_else(|e| e.into_inner());
        let Some(l) = layers.get(&layer) else { return };
        for &(data, stride, n) in &l.tensors {
            for &e in experts.iter().filter(|e| **e < n) {
                let start = data + e * stride;
                let aligned = start & !(self.page - 1);
                will_need(aligned, start + stride - aligned);
                self.asked.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(unix)]
fn will_need(addr: usize, len: usize) {
    unsafe {
        libc::madvise(addr as *mut c_void, len, libc::MADV_WILLNEED);
    }
}

#[cfg(not(unix))]
fn will_need(_addr: usize, _len: usize) {}

fn name_of(t: &llama_cpp_sys_2::ggml_tensor) -> &str {
    unsafe { CStr::from_ptr(t.name.as_ptr()) }.to_str().unwrap_or("")
}

/// blk.<layer>.ffn_gate_exps.weight (and up, down, gate_up): the layer, else None.
fn expert_weight_layer(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("blk.")?;
    let (layer, tail) = rest.split_once('.')?;
    matches!(tail, "ffn_gate_exps.weight" | "ffn_up_exps.weight" | "ffn_down_exps.weight" | "ffn_gate_up_exps.weight").then(|| layer.parse().ok()).flatten()
}

/// The callback llama.cpp calls for each node of the graph. user_data is the runner's Prefetch.
///
/// # Safety
/// llama.cpp passes a live tensor and the user data given to the context, which outlives it.
pub unsafe extern "C" fn eval_callback(t: *mut llama_cpp_sys_2::ggml_tensor, ask: bool, user_data: *mut c_void) -> bool {
    if t.is_null() || user_data.is_null() {
        return !ask; // nothing to see: let the computation go on
    }
    let p = unsafe { &*(user_data as *const Prefetch) };
    let t = unsafe { &*t };
    let name = name_of(t);
    if ask {
        // the experts' weights are the sources of the expert matrix products: their places are learned once
        for s in t.src.iter().filter(|s| !s.is_null()) {
            let s = unsafe { &**s };
            if let Some(layer) = expert_weight_layer(name_of(s)) {
                if !s.data.is_null() && s.ne[2] > 0 {
                    p.record(layer, s.data as usize, s.nb[2], s.ne[2] as usize);
                }
            }
        }
        return name.starts_with("ffn_moe_topk-");
    }
    if let Some(layer) = name.strip_prefix("ffn_moe_topk-").and_then(|n| n.parse::<u32>().ok()) {
        // the chosen experts: int32 [experts used, tokens], perhaps a view, so read by its strides. While a prompt is
        // read, almost every expert is needed anyway: only the few tokens of writing an answer are worth it.
        let (used, tokens) = (t.ne[0].max(0) as usize, t.ne[1].max(0) as usize);
        if !t.data.is_null() && t.type_ == llama_cpp_sys_2::GGML_TYPE_I32 && tokens <= 4 && used <= 64 {
            let mut chosen = Vec::with_capacity(used * tokens);
            for j in 0..tokens {
                for i in 0..used {
                    let at = t.data as usize + i * t.nb[0] + j * t.nb[1];
                    let e = unsafe { *(at as *const i32) };
                    if e >= 0 && !chosen.contains(&(e as usize)) {
                        chosen.push(e as usize);
                    }
                }
            }
            p.want(layer, &chosen);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(expert_weight_layer("blk.12.ffn_gate_exps.weight"), Some(12));
        assert_eq!(expert_weight_layer("blk.0.ffn_gate_up_exps.weight"), Some(0));
        assert_eq!(expert_weight_layer("blk.3.ffn_gate_shexp.weight"), None);
        assert_eq!(expert_weight_layer("output.weight"), None);
    }
}
