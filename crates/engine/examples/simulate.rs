use pokerdraft_engine::*;
use sha2::{Digest,Sha256};
fn main(){
 let count=std::env::args().nth(1).and_then(|s|s.parse::<u64>().ok()).unwrap_or(1000);
 let start=std::time::Instant::now();let mut scores=[0u64;6];let mut cashes=0;let mut burns=0;
 for seed in 0..count {
  let mut g=Game::new(Rules::default(),(0..6).map(|i|Player::new(i.to_string(),format!("Baseline {i}"),true)).collect(),Sha256::digest(seed.to_le_bytes()).into()).unwrap();
  for _ in 0..52 {for seat in 0..6{play_house(&mut g,seat);}g.settle().unwrap();}
  for (i,p) in g.players.iter().enumerate(){scores[i]+=p.score as u64;}
  cashes+=g.events.iter().filter(|e|e.kind=="cash_in").count();burns+=g.events.iter().filter(|e|e.kind=="burn").count();
 }
 println!("{}",serde_json::json!({"matches":count,"elapsed_ms":start.elapsed().as_millis(),"score_totals":scores,"cash_ins":cashes,"burns":burns}));
 let cards=[0,1,2,3,4,12,25,38,51,20];let start=std::time::Instant::now();for _ in 0..10000{std::hint::black_box(poker::best(std::hint::black_box(&cards)));}println!("best_of_ten_ns: {}",start.elapsed().as_nanos()/10000);
}
