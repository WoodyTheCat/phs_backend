use axum::Router;

mod category;
mod department;
mod post;
mod user;

pub use department::Department;
pub use user::User;

pub fn router() -> Router {
    Router::new()
        .merge(user::router())
        .merge(post::router())
        .merge(category::router())
        .merge(department::router())
}
