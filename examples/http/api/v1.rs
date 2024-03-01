use std::collections::HashMap;

#[netfn::service]
pub trait TestApi {
    async fn foo(&self);
    async fn bar(&self, inp: bool) -> ();
    async fn baz(&self) -> u32;
    async fn qaz(&self, inp: String) -> Vec<String>;
    async fn qoz(&self, inp: HashMap<String, String>, val: i16) -> Result<bool, String>;
}

/*
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
*/

pub fn mod_path() -> &'static str {
    module_path!()
}
