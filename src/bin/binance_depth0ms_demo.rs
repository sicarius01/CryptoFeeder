use futures_util::StreamExt;
use std::env;
use std::time::Duration;
use tokio::time::{sleep, Instant, sleep_until};
use futures_util::FutureExt;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	// 인자: 실행 시간(초). 없으면 무한 실행
	let args: Vec<String> = env::args().collect();
	let timeout_secs: Option<u64> = if args.len() >= 2 { args[1].parse::<u64>().ok().filter(|v| *v > 0) } else { None };

	// 테스트 심볼 (Futures USDT 계약) - 여러 개 가능
	let symbols = vec!["btcusdt", "ethusdt"];
    let streams: Vec<String> = symbols
        .iter()
        .flat_map(|s| vec![format!("{}@depth@0ms", s)])
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

	if let Some(t) = timeout_secs {
		let deadline = start + Duration::from_secs(t);
		let mut timer = sleep_until(deadline);
		tokio::pin!(timer);
		loop {
			let next_msg = read.next().fuse();
			tokio::pin!(next_msg);
			tokio::select! {
				_ = &mut timer => { println!("⏰ timeout reached ({}s)", t); break; }
				msg = &mut next_msg => {
					match msg {
						Some(Ok(Message::Text(txt))) => {
							println!("📥 {} bytes", txt.len());
                    if let Some(sym) = extract_symbol_from_combined(&txt) {
                        println!("   stream for symbol: {} (depth@0ms)", sym);
                    }
						}
						Some(Ok(Message::Binary(bin))) => { println!("📥 [bin] {} bytes", bin.len()); }
						Some(Ok(Message::Ping(p))) => { println!("🏓 ping ({} bytes)", p.len()); }
						Some(Ok(Message::Pong(_))) => { }
						Some(Ok(Message::Frame(_))) => { }
						Some(Ok(Message::Close(_))) => { println!("🔚 closed by server"); break; }
						Some(Err(e)) => { eprintln!("❌ ws error: {}", e); break; }
						None => { println!("🔚 stream ended"); break; }
					}
				}
			}
		}
	} else {
		loop {
			match read.next().await {
			Some(Ok(Message::Text(txt))) => {
				// depth0ms 메시지 일부만 간단 표기
				println!("📥 {} bytes", txt.len());
                if let Some(sym) = extract_symbol_from_combined(&txt) { println!("   stream for symbol: {} (depth@0ms)", sym); }
			}
			Some(Ok(Message::Binary(bin))) => {
				println!("📥 [bin] {} bytes", bin.len());
			}
			Some(Ok(Message::Ping(p))) => {
				println!("🏓 ping ({} bytes)", p.len());
			}
			Some(Ok(Message::Pong(_))) => {}
			Some(Ok(Message::Frame(_))) => {}
			Some(Ok(Message::Close(_))) => { println!("🔚 closed by server"); break; }
			Some(Err(e)) => { eprintln!("❌ ws error: {}", e); break; }
			None => { println!("🔚 stream ended"); break; }
		}

		// 과도한 출력 방지
		sleep(Duration::from_millis(5)).await;
	}
	}

	Ok(())
}

// Binance combined stream 텍스트에서 심볼 추출 (가벼운 JSON 스캔)
fn extract_symbol_from_combined(txt: &str) -> Option<String> {
	// 예: {"stream":"btcusdt@depth0ms","data":{..."s":"BTCUSDT",...}}
	if let Some(pos) = txt.find("\"stream\":\"") {
		let rest = &txt[pos + 10..];
		if let Some(end) = rest.find('\"') {
			let stream = &rest[..end];
			if let Some(at) = stream.find('@') { return Some(stream[..at].to_string()); }
		}
	}
	None
}


