use std::future::Future;

use ulid::Ulid;

use crate::{AppState, Item};

#[netfn::service]
pub trait TestApi {
    async fn foo(&self, input: String) -> Item;
    async fn bar(&self, item: Item, input: String);
    // fn b(&self, input: String) -> impl Future<Output = Item> + Send;
}

pub trait ItemApi {
    // Auto-generated methods

    type State;
    fn new(state: Self::State) -> Self;
    // Has a pre-written body based on the user-given methods
    fn dispatch(&self, request: ()) -> impl Future<Output = ()> + Send;

    // User-given methods
    // When written by a user, only the `Output` type is needed, the rest is auto-generated

    fn create(&self, content: String) -> impl Future<Output = Item> + Send;
    fn list(&self) -> impl Future<Output = Vec<Item>> + Send;
    fn update(&self, id: &str, content: String) -> impl Future<Output = bool> + Send;
    fn delete(&self, id: &str) -> impl Future<Output = bool> + Send;
}

#[derive(Clone)]
pub struct ItemService {
    state: AppState,
}

impl ItemApi for ItemService {
    type State = AppState;

    fn new(state: Self::State) -> Self {
        Self { state }
    }

    async fn dispatch(&self, request: ()) -> () {
        ()
    }

    async fn create(&self, content: String) -> Item {
        let item = Item {
            id: Ulid::new().to_string(),
            content,
        };
        self.state.insert(item.id.clone(), item.clone());
        item
    }

    async fn list(&self) -> Vec<Item> {
        self.state.list()
    }

    async fn update(&self, id: &str, content: String) -> bool {
        self.state.update(id.to_owned(), content)
    }

    async fn delete(&self, id: &str) -> bool {
        self.state.delete(id)
    }
}

pub fn mod_path() -> &'static str {
    module_path!()
}
