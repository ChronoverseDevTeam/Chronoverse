use crv_core::logger::test;

#[tokio::main]
async fn main() {
    test::run_all_tests().await;
}

