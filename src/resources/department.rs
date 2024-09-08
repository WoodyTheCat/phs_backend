use axum::{
    extract::Path,
    routing::{delete, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, PgPool};
use tracing::instrument;

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
        .route(
            "/v1/departments",
            post(create_department).get(get_departments),
        )
}

#[instrument(skip(pool))]
async fn get_departments(
    Extension(pool): Extension<PgPool>,
) -> Result<Json<Vec<Department>>, PhsError> {
    let departments = sqlx::query_as!(
        Department,
        r#"SELECT id, department FROM departments LIMIT 100"#
    )
    .fetch_all(&pool)
    .await?;

    Ok(Json(departments))
}

#[instrument(skip(pool))]
async fn get_department(
    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<Json<Department>, PhsError> {
    let department = sqlx::query_as!(
        Department,
        r#"SELECT id, department FROM departments WHERE id = $1"#,
        id
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(department))
}

#[derive(Deserialize, Debug)]
struct CreateDepartmentBody {
    department: String,
}

#[instrument(skip(pool, _auth_session))]
async fn create_department(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditDepartments as u8 }>,

    Extension(pool): Extension<PgPool>,
    Json(req): Json<CreateDepartmentBody>,
) -> Result<Json<Department>, PhsError> {
    let department = sqlx::query_as!(
        Department,
        r#"
        INSERT INTO departments(department)
        VALUES ($1)
        RETURNING id, department
        "#,
        req.department
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(department))
}

#[derive(Deserialize, Debug)]
struct PutDepartmentBody {
    new: String,
}

#[instrument(skip(pool, _auth_session))]
async fn put_department(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditDepartments as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
    Json(body): Json<PutDepartmentBody>,
) -> Result<Json<Department>, PhsError> {
    let department = sqlx::query_as!(
        Department,
        r#"
        UPDATE departments
        SET department = $1
        WHERE id = $2
        RETURNING id, department
        "#,
        body.new,
        id
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(department))
}

#[instrument(skip(pool, _auth_session))]
async fn delete_department(
    _auth_session: AuthSession,
    _: RequirePermission<{ Permission::EditDepartments as u8 }>,

    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<(), PhsError> {
    sqlx::query!(r#"DELETE FROM departments WHERE id = $1"#, id)
        .fetch_one(&pool)
        .await?;

    Ok(())
}
