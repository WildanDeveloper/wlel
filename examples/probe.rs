fn main(){
    let a = r#""hi\nthere \"x\""#;
    println!("raw  bytes: {:?}", a);
    let b = "\"hi\\nthere \\\"x\\\"\"";
    println!("esc  bytes: {:?}", b);
    println!("equal: {}", a == b);
}
