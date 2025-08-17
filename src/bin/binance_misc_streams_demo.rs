use futures_util::StreamExt;
use std::env;
use std::time::Duration;
use tokio::time::{sleep, Instant, sleep_until};
use futures_util::FutureExt;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use serde_json::Value;
use crypto_feeder::packet_builder::PacketBuilder;
use crypto_feeder::udp_broadcaster::UdpMulticaster;
use crypto_feeder::protocol::{MESSAGE_TYPE_INDEX_PRICE, MESSAGE_TYPE_MARK_PRICE, MESSAGE_TYPE_FUNDING_RATE};
use crypto_feeder::config::Config;

/// Binance Futures WebSocket demo for index price, mark price, and liquidation streams.
/// - Connects to combined streams on `wss://fstream.binance.com/stream`
/// - Default symbols: btcusdt, ethusdt (lowercase, per Binance stream naming)
/// - Optional arg[1]: timeout seconds (u64). If provided and > 0, auto-stop after N seconds
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	// arg: optional timeout seconds
	let args: Vec<String> = env::args().collect();
	let timeout_secs: Option<u64> = if args.len() >= 2 { args[1].parse::<u64>().ok().filter(|v| *v > 0) } else { None };

	// target symbols (UM futures). Keep lowercase for stream path
	let symbols = vec!["btcusdt", "ethusdt"];
	let streams: Vec<String> = symbols
		.iter()
		.flat_map(|s| vec![
			// NOTE: On USDⓈ-M (fstream), standalone @indexPrice stream is not provided.
			// Index price is included within markPriceUpdate as field "i". Use @markPrice@1s for 1s cadence.
			format!("{}@markPrice@1s", s),
			format!("{}@forceOrder", s),
		])
		.collect();

	let url = format!(
		"wss://fstream.binance.com/stream?streams={}",
		streams.join("/")
	);

	println!("🔗 Connect: {}", url);
    let (ws_stream, resp) = connect_async(&url).await?;
	println!("🤝 Connected: status={}", resp.status());

	let (_write, mut read) = ws_stream.split();
	let start = Instant::now();

    // UDP 송신기 준비
    let cfg = Config::load().unwrap_or_else(|_| Config {
        exchanges: vec![], symbols: vec![],
        udp: crypto_feeder::config::UdpConfig { multicast_addr: "239.255.1.1".into(), port: 55555, interface_addr: "0.0.0.0".into() },
        logging: crypto_feeder::config::LoggingConfig { level: "info".into(), file_path: None },
        runtime_threads: None, metrics: crypto_feeder::config::MetricsConfig { enabled: false, interval_secs: 5 },
        symbol_config: None, endpoint_config: None,
    });
    let udp = UdpMulticaster::new(&cfg.udp).expect("UDP 초기화 실패");
    let builder = PacketBuilder::new();

    if let Some(t) = timeout_secs {
		let deadline = start + Duration::from_secs(t);
        let timer = sleep_until(deadline);
		tokio::pin!(timer);
		loop {
			let next_msg = read.next().fuse();
			tokio::pin!(next_msg);
			tokio::select! {
				_ = &mut timer => { println!("⏰ timeout reached ({}s)", t); break; }
				msg = &mut next_msg => {
                    match msg {
                        Some(Ok(Message::Text(txt))) => handle_text_message(&txt, &builder, &udp).await,
						Some(Ok(Message::Binary(bin))) => { println!("📥 [bin] {} bytes", bin.len()); },
						Some(Ok(Message::Ping(p))) => { println!("🏓 ping ({} bytes)", p.len()); },
						Some(Ok(Message::Pong(_))) => { },
						Some(Ok(Message::Frame(_))) => { },
						Some(Ok(Message::Close(_))) => { println!("🔚 closed by server"); break; },
						Some(Err(e)) => { eprintln!("❌ ws error: {}", e); break; },
						None => { println!("🔚 stream ended"); break; },
					}
				}
			}
		}
	} else {
		loop {
			match read.next().await {
                Some(Ok(Message::Text(txt))) => handle_text_message(&txt, &builder, &udp).await,
				Some(Ok(Message::Binary(bin))) => { println!("📥 [bin] {} bytes", bin.len()); },
				Some(Ok(Message::Ping(p))) => { println!("🏓 ping ({} bytes)", p.len()); },
				Some(Ok(Message::Pong(_))) => { },
				Some(Ok(Message::Frame(_))) => { },
				Some(Ok(Message::Close(_))) => { println!("🔚 closed by server"); break; },
				Some(Err(e)) => { eprintln!("❌ ws error: {}", e); break; },
				None => { println!("🔚 stream ended"); break; },
			}

			// reduce console flood
			sleep(Duration::from_millis(5)).await;
		}
	}

    Ok(())
}

async fn handle_text_message(txt: &str, builder: &PacketBuilder, udp: &UdpMulticaster) {
	println!("📥 {} bytes", txt.len());
	if let Ok(v) = serde_json::from_str::<Value>(txt) {
		let stream_name = v.get("stream").and_then(|s| s.as_str()).unwrap_or("");
		let data = v.get("data").cloned().unwrap_or(Value::Null);
		let event_type = data.get("e").and_then(|e| e.as_str()).unwrap_or("");
		match event_type {
			"markPriceUpdate" => {
				let sym = data.get("s").and_then(|x| x.as_str()).unwrap_or("");
				let mark = data.get("p").and_then(|x| x.as_str()).unwrap_or("");
				let index = data.get("i").and_then(|x| x.as_str()).unwrap_or("");
				let funding = data.get("r").and_then(|x| x.as_str()).unwrap_or("");
				println!("   [markPrice] stream={} s={} mark={} index={} fundingRate={}", stream_name, sym, mark, index, funding);

                // UDP: index price (2), mark price (3), funding rate (4)
                if let (Ok(mark_f), Ok(index_f), Ok(funding_f)) = (
                    mark.parse::<f64>(), index.parse::<f64>(), funding.parse::<f64>()
                ) {
                    // 표준 심볼/거래소 이름
                    let symbol_std = to_std_symbol(sym);
                    let exchange_std = "BinanceFutures";
                    let ts = data.get("E").and_then(|x| x.as_u64()).unwrap_or(0);

                    // scale 적용
                    let mark_scaled = (mark_f * crypto_feeder::protocol::PRICE_SCALE as f64) as i64;
                    let index_scaled = (index_f * crypto_feeder::protocol::PRICE_SCALE as f64) as i64;
                    let funding_scaled = (funding_f * crypto_feeder::protocol::FUNDING_RATE_SCALE as f64) as i64;

                    if let Ok(pkt) = builder.build_single_value_packet(&symbol_std, exchange_std, MESSAGE_TYPE_INDEX_PRICE, ts, index_scaled) {
                        let _ = udp.send_packet(pkt).await;
                    }
                    if let Ok(pkt) = builder.build_single_value_packet(&symbol_std, exchange_std, MESSAGE_TYPE_MARK_PRICE, ts, mark_scaled) {
                        let _ = udp.send_packet(pkt).await;
                    }
                    if let Ok(pkt) = builder.build_single_value_packet(&symbol_std, exchange_std, MESSAGE_TYPE_FUNDING_RATE, ts, funding_scaled) {
                        let _ = udp.send_packet(pkt).await;
                    }
                }
			}
			"indexPriceUpdate" => {
				let sym = data.get("s").and_then(|x| x.as_str()).unwrap_or("");
				let index = data.get("i").and_then(|x| x.as_str()).unwrap_or("");
				println!("   [indexPrice] stream={} s={} index={}", stream_name, sym, index);
			}
			"forceOrder" => {
				let o = data.get("o").cloned().unwrap_or(Value::Null);
				let sym = o.get("s").and_then(|x| x.as_str()).unwrap_or("");
				let side = o.get("S").and_then(|x| x.as_str()).unwrap_or("");
				let price = o.get("ap").or_else(|| o.get("p")).and_then(|x| x.as_str()).unwrap_or("");
				let qty = o.get("q").and_then(|x| x.as_str()).unwrap_or("");
				println!("   [liquidation] stream={} s={} side={} price={} qty={}", stream_name, sym, side, price, qty);

                if let (Ok(price_f), Ok(qty_f)) = (price.parse::<f64>(), qty.parse::<f64>()) {
                    let symbol_std = to_std_symbol(sym);
                    let exchange_std = "BinanceFutures";
                    let ts = data.get("E").and_then(|x| x.as_u64()).unwrap_or(0);
                    let is_sell = side.eq_ignore_ascii_case("SELL");
                    if let Ok(pkt) = builder.build_liquidation_packet(&symbol_std, exchange_std, ts, price_f, qty_f, is_sell) {
                        let _ = udp.send_packet(pkt).await;
                    }
                }
			}
			_ => {
				// Unknown or different event; show stream key
				if !stream_name.is_empty() { println!("   stream={}", stream_name); }
			}
		}
	} else {
		// Fallback: try to extract `stream` prefix like depth demo
        if let Some(pos) = txt.find("\"stream\":\"") {
			let rest = &txt[pos + 10..];
			if let Some(end) = rest.find('"') { println!("   stream={}", &rest[..end]); }
		}
	}
}

fn to_std_symbol(binance_symbol: &str) -> String {
    // ex) BTCUSDT -> BTC^USDT
    if binance_symbol.len() >= 6 {
        let (base, quote) = binance_symbol.split_at(binance_symbol.len() - 4);
        format!("{}^{}", base, quote)
    } else {
        binance_symbol.to_string()
    }
}


