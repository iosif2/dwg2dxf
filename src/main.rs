use axum::{
    Json, Router,
    body::Body,
    extract::Multipart,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_openapi3::utoipa::OpenApi;
use axum_openapi3::{AddRoute, reset_openapi};
use clap::{Parser, arg};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tracing::info;
use tracing_subscriber::prelude::*;
use utoipa::ToSchema;
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

// 홈페이지 핸들러
async fn home() -> &'static str {
    "DWG to DXF Converter API\n\nEndpoints:\n- POST /convert - Upload DWG file to convert to DXF"
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(long = "host", short = 'H', default_value = "0.0.0.0")]
    pub host: String,

    #[arg(long = "port", short = 'P', default_value = "3000")]
    pub port: u16,
}

#[derive(Deserialize, ToSchema)]
pub struct ConvertRequest {
    #[schema(format = "binary")]
    pub file: String, // 또는 bytes Vec<u8> 등
}

#[utoipa::path(
    post,
    path = "/convert",
    description = "Convert DWG file to DXF format",
    request_body(content = ConvertRequest, content_type = "multipart/form-data", description = "DWG file to convert"),
    responses(
        (status = 200, description = "Successfully converted DXF file", content_type = "application/octet-stream"),
        (status = 400, description = "Bad request - invalid file or missing parameters"),
        (status = 500, description = "Internal server error - conversion failed")
    )
)]
async fn convert_dwg_to_dxf(mut multipart: Multipart) -> Result<impl IntoResponse, AppError> {
    let mut dwg_file_path: Option<PathBuf> = None;

    // boundary가 올바르게 전달되지 않은 경우 에러 반환
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        if e.to_string().contains("No boundary found") {
            AppError::BadRequest("multipart/form-data 요청에 boundary가 필요합니다.".to_string())
        } else {
            AppError::BadRequest(e.to_string())
        }
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            let filename = field
                .file_name()
                .ok_or_else(|| AppError::BadRequest("No filename provided".to_string()))?
                .to_string();

            if !filename.to_lowercase().ends_with(".dwg") {
                return Err(AppError::BadRequest("File must be a .dwg file".to_string()));
            }

            let data = field
                .bytes()
                .await
                .map_err(|e| AppError::BadRequest(e.to_string()))?;

            // 임시 파일 생성
            // 임시 파일을 .dwg 확장자로 생성
            // 네, uuidv4를 사용해서 임시 파일명을 지정할 수 있습니다.
            let uuid = Uuid::new_v4().to_string();
            let temp_file_path = std::env::temp_dir().join(format!("{}.dwg", uuid));
            let mut temp_file = std::fs::File::create(&temp_file_path).map_err(|e| {
                AppError::InternalServerError(format!("Failed to create temp file: {}", e))
            })?;

            // 파일 데이터 쓰기
            std::io::Write::write_all(&mut temp_file, &data).map_err(|e| {
                AppError::InternalServerError(format!("Failed to write file: {}", e))
            })?;

            dwg_file_path = Some(temp_file_path);
            break;
        }
    }

    let dwg_path =
        dwg_file_path.ok_or_else(|| AppError::BadRequest("No DWG file provided".to_string()))?;
    // DXF 출력 파일 경로 생성
    let output_id = Uuid::new_v4().to_string();
    // 임시 폴더를 사용하여 DXF 파일 경로 생성
    let dxf_filename = format!("{}.dxf", output_id);
    let dxf_path = std::env::temp_dir().join(&dxf_filename);

    // dwg2dxf 명령어 실행
    let output = Command::new("/usr/local/bin/dwg2dxf")
        .arg("-o")
        .arg(&dxf_path)
        .arg(&dwg_path)
        .output()
        .map_err(|e| AppError::InternalServerError(format!("Failed to execute dwg2dxf: {}", e)))?;

    // 임시 파일 정리

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::InternalServerError(format!(
            "Conversion failed: {}",
            error_msg
        )));
    }

    // 변환 성공 확인
    if !dxf_path.exists() {
        return Err(AppError::InternalServerError(
            "DXF file was not created".to_string(),
        ));
    }

    // 파일을 읽어서 메모리에 저장한 후 정리
    let file_content = tokio::fs::read(&dxf_path).await.map_err(|e| {
        AppError::InternalServerError(format!("DXF 파일을 읽을 수 없습니다: {}", e))
    })?;

    info!(
        "DXF Conversion successful, dwg file path: {:?} dxf file path: {:?}",
        dwg_path, dxf_path
    );

    // 파일 정리
    let _ = fs::remove_file(&dwg_path);
    let _ = fs::remove_file(&dxf_path);

    let filename = dxf_filename.clone();
    let content_disposition = format!("attachment; filename=\"{}\"", filename);

    Ok(Response::builder()
        .header("Content-Type", "application/octet-stream")
        .header("Content-Disposition", content_disposition)
        .body(Body::from(file_content))
        .unwrap())
}

// 에러 처리
#[derive(Debug)]
enum AppError {
    BadRequest(String),
    InternalServerError(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            AppError::InternalServerError(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };

        (status, error_message).into_response()
    }
}

#[derive(OpenApi)]
#[openapi(paths(convert_dwg_to_dxf, openapi))]
struct ApiDoc;

#[utoipa::path(
    get,
    path = "/openapi.json",
    description = "Get OpenAPI specification",
    responses(
        (status = 200, description = "OpenAPI specification", content_type = "application/json")
    )
)]
async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

fn get_router() -> Router {
    Router::new()
        .add(("/convert", post(convert_dwg_to_dxf)))
        .add(("/openapi.json", get(openapi)))
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    reset_openapi();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app = Router::new()
        .route("/", get(home))
        .merge(get_router())
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()));

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.host, args.port))
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
