pub mod mongo;

pub use mongo::{
    get_mongo,
    init_mongo_from_config,
    init_mongo_with_config,
    MongoManager,
};

