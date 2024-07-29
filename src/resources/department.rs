use axum::{
    extract::Path,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, PgPool};

use crate::{
    auth::{AuthSession, Permission, RequirePermission},
    error::PhsError,
};

#[derive(FromRow, Serialize, Deserialize)]
pub struct Department {
    pub id: i32,
    pub department: String,
}

pub fn router() -> Router {
    Router::new()
        .route(
            "/v1/department/:id",
            delete(delete_department)
                .put(put_department)
                .get(get_department),
        )
        .route("/v1/departments", get(get_departments))
        .route("/v1/department", post(create_department))
}

async fn get_departments(
    Extension(pool): Extension<PgPool>,
) -> Result<Json<Vec<Department>>, PhsError> {
    let departments = sqlx::query_as!(Department, "SELECT * FROM departments LIMIT 100")
        .fetch_all(&pool)
        .await?;

    Ok(Json(departments))
}

async fn get_department(
    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<Json<Department>, PhsError> {
    let department = sqlx::query_as!(Department, r#"SELECT * FROM departments WHERE id = $1"#, id)
        .fetch_one(&pool)
        .await?;

    Ok(Json(department))
}

#[derive(Deserialize)]
struct CreateDepartmentBody {
    department: String,
}

async fn create_department(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditDepartments as i32 }>,

    Extension(pool): Extension<PgPool>,
    Json(req): Json<CreateDepartmentBody>,
) -> Result<Json<Department>, PhsError> {
    let department = sqlx::query_as!(
        Department,
        r#"INSERT INTO departments(department) VALUES ($1) RETURNING *"#,
        req.department
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(department))
}

#[derive(Deserialize)]
struct PutDepartmentBody {
    new: String,
}

async fn put_department(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditDepartments as i32 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
    Json(body): Json<PutDepartmentBody>,
) -> Result<Json<Department>, PhsError> {
    let department = sqlx::query_as!(
        Department,
        r#"UPDATE departments SET department = $1 WHERE id = $2 RETURNING *"#,
        body.new,
        id
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(department))
}

async fn delete_department(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditDepartments as i32 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<(), PhsError> {
    sqlx::query!(r#"DELETE FROM departments WHERE id = $1"#, id)
        .fetch_one(&pool)
        .await?;

    Ok(())
}
