use crate::{
    error::PhsError,
    resources::{CursorPaginatable, HasSqlxQueryString, SqlxQueryString},
};
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, QueryBuilder};

use super::AuthSession;

#[derive(PartialEq, Eq, Clone, Copy, Deserialize, Serialize, Debug, sqlx::Type)]
#[sqlx(type_name = "permission", rename_all = "snake_case")]
pub enum Permission {
    EditDepartments = 0,
    EditCategories,
    CreatePosts,
    EditPosts,
    ManageUsers,
    ManagePermissions,
    ManagePages,
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::EditDepartments => "EditDepartments",
                Self::EditCategories => "EditCategories",
                Self::CreatePosts => "CreatePosts",
                Self::EditPosts => "EditPosts",
                Self::ManageUsers => "ManageUsers",
                Self::ManagePermissions => "ManagePermissions",
                Self::ManagePages => "ManagePages",
            }
        )
    }
}

impl TryFrom<u8> for Permission {
    type Error = ();
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::EditDepartments),
            1 => Ok(Self::EditCategories),
            2 => Ok(Self::CreatePosts),
            3 => Ok(Self::EditPosts),
            4 => Ok(Self::ManageUsers),
            5 => Ok(Self::ManagePermissions),
            6 => Ok(Self::ManagePages),
            _ => Err(()),
        }
    }
}

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

        let required_permission: Permission = PERMISSION
            .try_into()
            .expect("Unexpected integer for Permission in RequirePermission");

        auth_session
            .data()
            .permissions
            .contains(&required_permission)
            .then_some(Self)
            .ok_or(PhsError(StatusCode::FORBIDDEN, None, "Missing permission"))
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

    fn order_by_clause<'a>(&'a self, builder: &mut QueryBuilder<'a, sqlx::Postgres>) -> bool {
        let Some((field, order)) = Self::parse_sort_by(&self.sort_by) else {
            return false;
        };

        if let s @ ("id" | "group_name") = field.as_str() {
            builder.push(s);
            order.append_to(builder);
            true
        } else {
            false
        }
    }
}

impl CursorPaginatable for Group {
    fn id(&self) -> i32 {
        self.id
    }
}

#[derive(Clone, FromRow, Serialize, Deserialize, Debug)]
pub struct UserPermissions {
    pub id: i32,
    pub username: String,
    pub name: String,
    pub permissions: Vec<Permission>,
    pub group_ids: Vec<i32>,
}

impl HasSqlxQueryString for UserPermissions {
    type QueryString = UserPermissionsQueryString;
}

#[derive(Deserialize, Debug)]
pub struct UserPermissionsQueryString {
    username: Option<String>,
    name: Option<String>,
    permissions: Option<Vec<Permission>>,
    group_ids: Option<Vec<i32>>,
    sort_by: Option<String>,
}

impl SqlxQueryString for UserPermissionsQueryString {
    fn where_clause<'a>(&'a self, builder: &mut QueryBuilder<'a, sqlx::Postgres>) {
        if let Some(ref username) = self.username {
            builder.push(" AND username LIKE ");
            builder.push_bind(username);
        }

        if let Some(ref name) = self.name {
            builder.push(" AND name LIKE ");
            builder.push_bind(name);
        }

        if let Some(ref permissions) = self.permissions {
            builder.push(" AND permissions @> ");
            builder.push_bind(permissions);
        }

        if let Some(ref group_ids) = self.group_ids {
            builder.push(" AND group_ids @> ");
            builder.push_bind(group_ids);
        }
    }

    fn order_by_clause<'a>(&'a self, builder: &mut QueryBuilder<'a, sqlx::Postgres>) -> bool {
        let Some((field, order)) = Self::parse_sort_by(&self.sort_by) else {
            return false;
        };

        if let s @ ("id" | "group_name") = field.as_str() {
            builder.push(s);
            order.append_to(builder);
            true
        } else {
            false
        }
    }
}

impl CursorPaginatable for UserPermissions {
    fn id(&self) -> i32 {
        self.id
    }
}
