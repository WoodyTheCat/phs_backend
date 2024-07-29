use crate::error::PhsError;
use axum::{async_trait, extract::FromRequestParts, http::request::Parts};
use serde::{Deserialize, Serialize};
use sqlx::{
    postgres::{PgHasArrayType, PgTypeInfo},
    prelude::FromRow,
};

use super::AuthSession;

macro_rules! i32_to_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident {
        $($(#[$vmeta:meta])* $vname:ident $(= $val:expr)?,)*
    }) => {
        $(#[$meta])*
        $vis enum $name {
            $($(#[$vmeta])* $vname $(= $val)?,)*
        }

        impl std::convert::TryFrom<i32> for $name {
            type Error = ();

            fn try_from(v: i32) -> Result<Self, Self::Error> {
                match v {
                    $(x if x == $name::$vname as i32 => Ok($name::$vname),)*
                    _ => Err(()),
                }
            }
        }
    }
}

i32_to_enum!(
    #[derive(PartialEq, Eq, Clone, sqlx::Type, Deserialize, Serialize)]
    #[sqlx(type_name = "permission", rename_all = "snake_case")]
    pub enum Permission {
        EditDepartments,
        EditCategories,
        CreatePosts,
        EditPosts,
        ManageUsers,
        ManagePermissions,
        CreatePages,
    }
);

impl PgHasArrayType for Permission {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        PgTypeInfo::with_name("_permission")
    }
}

pub struct RequirePermission<const PERMISSION: i32>;

#[async_trait]
impl<S, const PERMISSION: i32> FromRequestParts<S> for RequirePermission<PERMISSION> {
    type Rejection = PhsError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_session = parts
            .extensions
            .get::<AuthSession>()
            .ok_or(PhsError::UNAUTHORIZED)?;

        auth_session
            .user()
            .permissions
            .contains(&PERMISSION.try_into().map_err(|_| PhsError::INTERNAL)?)
            .then_some(Self)
            .ok_or(PhsError::FORBIDDEN)
    }
}

#[derive(Clone, FromRow, Serialize, Deserialize)]
pub struct Group {
    pub id: i32,
    pub group_name: String,
    pub permissions: Vec<Permission>,
}
