use crate::{
    error::PhsError,
    resources::{HasSqlxQueryString, SqlxQueryString},
    CursorPaginatable,
};
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, QueryBuilder};

use super::AuthSession;

macro_rules! i32_to_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident {
        $($(#[$vmeta:meta])* $vname:ident $(= $val:expr)?,)*
    }) => {
        $(#[$meta])*
        $vis enum $name {
            $($(#[$vmeta])* $vname $(= $val)?,)*
        }

        impl From<u8> for $name {
            fn from(i: u8) -> Self {
                match i {
                    $(i if i == Self::$vname as u8 => Self::$vname,)*
                    _ => panic!("Conversion from i32 to Permission failed, but all inputs are hardcoded!"),
                }
            }
        }
    }
}

i32_to_enum!(
    #[derive(PartialEq, Eq, Clone, Copy, sqlx::Type, Deserialize, Serialize, Debug)]
    #[non_exhaustive]
    #[sqlx(type_name = "permission", rename_all = "snake_case")]
    pub enum Permission {
        EditDepartments,
        EditCategories,
        CreatePosts,
        EditPosts,
        ManageUsers,
        ManagePermissions,
        ManagePages,
    }
);

pub struct RequirePermission<const PERMISSION: u8>;

#[async_trait]
impl<S, const PERMISSION: u8> FromRequestParts<S> for RequirePermission<PERMISSION> {
    type Rejection = PhsError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_session = parts.extensions.get::<AuthSession>().ok_or(PhsError(
            StatusCode::UNAUTHORIZED,
            None,
            "Could not find AuthSession in request extensions",
        ))?;

        auth_session
            .data()
            .permissions
            .contains(&PERMISSION.into())
            .then_some(Self)
            .ok_or(PhsError(
                StatusCode::FORBIDDEN,
                None,
                "Inadequate permissions",
            ))
    }
}

#[derive(Clone, FromRow, Serialize, Deserialize, Debug)]
pub struct Group {
    pub id: i32,
    pub group_name: String,
    pub permissions: Vec<Permission>,
}

impl HasSqlxQueryString for Group {
    type QueryString = GroupQueryString;
}

#[derive(Deserialize, Debug)]
pub struct GroupQueryString {
    id: Option<i32>,
    group_name: Option<String>,
    sort_by: Option<String>,
}

impl SqlxQueryString for GroupQueryString {
    fn where_clause<'a>(&'a self, builder: &mut QueryBuilder<'a, sqlx::Postgres>) {
        if let Some(ref group_name) = self.group_name {
            builder.push(" AND group_name LIKE ");
            builder.push_bind(group_name);
        }

        if let Some(id) = self.id {
            builder.push(" AND id = ");
            builder.push_bind(id);
        }
    }

    fn order_by_clause<'a>(&'a self, builder: &mut QueryBuilder<'a, sqlx::Postgres>) {
        let Some((field, order)) = Self::parse_sort_by(&self.sort_by) else {
            return;
        };

        if let s @ ("id" | "group_name") = field.as_str() {
            builder.push(" ");
            builder.push(s);
            order.append_to(builder);
        }
    }
}

impl CursorPaginatable for Group {
    fn id(&self) -> i32 {
        self.id
    }
}
