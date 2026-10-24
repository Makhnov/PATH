#[path = "viewer/mod.rs"]
mod viewer;

use axum::{
    Json, Router,
    body::Body,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};

use viewer::{
    BenchConfigResponse, BenchResultQuery, BenchResultResponse, BenchStore, SnapshotOptions,
    ViewNode, ViewNodeRole, ViewSnapshot, VisualizeResultResponse, build_snapshot, build_view_node,
};

use lib_gps::{
    format::{Gps, reader::Reader},
    graph::{TileOrder, UGraph},
};

use serde::Deserialize;
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::services::ServeDir;

// Ajout pour la version swap cache
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use swap_cache::{CacheLRU, cache, storage, storages::MockFlash};

const CAPACITY: usize = 512; // 128Ko
const FLASH_SIZE: usize = 16_384; // 4Mo

// On crée un stockage externe
storage!(FLASH: MockFlash<FLASH_SIZE>);

// Et un cache (LRU)
cache!(CACHE: CacheLRU::<FLASH, CriticalSectionRawMutex, CAPACITY>);

#[derive(Clone)]
struct AppState {
    gps: Arc<Gps<CACHE>>,
    order: TileOrder,
    snapshot: Arc<ViewSnapshot>,
    bench: Arc<BenchStore>,
}

#[derive(Debug, Deserialize)]
struct SnapQuery {
    lon: f64,
    lat: f64,
    label: Option<String>,
    role: Option<ViewNodeRole>,
}

async fn snapshot(State(state): State<AppState>) -> Json<ViewSnapshot> {
    eprintln!(
        "[api] /api/snapshot -> segments={}, interests={}",
        state.snapshot.metrics.segments, state.snapshot.metrics.interests
    );

    Json((*state.snapshot).clone())
}

async fn bench_config(State(state): State<AppState>) -> Json<BenchConfigResponse> {
    Json(state.bench.config())
}

async fn bench_result(
    State(state): State<AppState>,
    Query(query): Query<BenchResultQuery>,
) -> Json<BenchResultResponse> {
    Json(state.bench.query(query))
}

async fn visualize_result(
    State(state): State<AppState>,
    Query(query): Query<BenchResultQuery>,
) -> Json<VisualizeResultResponse> {
    Json(state.bench.visualize(query))
}

async fn visualize_events(
    State(state): State<AppState>,
    Query(query): Query<BenchResultQuery>,
) -> Response {
    match state.bench.visualize_events_path(query) {
        Ok(Some(path)) => match std::fs::read(path) {
            Ok(bytes) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/x-ndjson; charset=utf-8")],
                Body::from(bytes),
            )
                .into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": err.to_string(),
                })),
            )
                .into_response(),
        },
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "no_data",
                "message": "Aucun artefact instrumenté disponible pour cette sélection.",
            })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": err.to_string(),
            })),
        )
            .into_response(),
    }
}

async fn snap(
    State(state): State<AppState>,
    Query(query): Query<SnapQuery>,
) -> Result<Json<ViewNode>, (StatusCode, String)> {
    eprintln!(
        "[api] /api/snap lon={} lat={} label={:?} role={:?}",
        query.lon, query.lat, query.label, query.role
    );

    let gps = state.gps.clone();
    let order = state.order;
    let lon = query.lon;
    let lat = query.lat;
    let label = query.label.unwrap_or_else(|| "Point".to_string());
    let role = query.role.unwrap_or(ViewNodeRole::Debug);

    let node = tokio::task::spawn_blocking(move || {
        let graph = embassy_futures::block_on(UGraph::new(gps.as_ref(), order)).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("failed to build graph: {err}"),
            )
        })?;

        embassy_futures::block_on(build_view_node(
            gps.as_ref(),
            &graph,
            lon,
            lat,
            &label,
            role,
        ))
    })
    .await
    .map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("snap task failed: {err}"),
        )
    })?
    .map_err(bad_request)?;

    Ok(Json(node))
}

#[tokio::main]
async fn main() -> IoResult<()> {
    unsafe {
        std::env::set_var("RUST_BACKTRACE", "1");
    }
    let _ = env_logger::builder()
        .format_timestamp(None)
        .parse_filters("info")
        .try_init();

    let config = ViewerConfig::from_args()?;

    let bytes = std::fs::read(&config.gps_path)?;
    let mut reader = Reader::new(&bytes);

    let gps = Gps::<CACHE>::new(&mut reader).await.map_err(|err| {
        IoError::new(
            ErrorKind::InvalidData,
            format!("échec du décodage du fichier GPS: {err}"),
        )
    })?;

    let snapshot_data = {
        let graph = UGraph::new(&gps, config.order).await.map_err(|err| {
            IoError::new(
                ErrorKind::InvalidData,
                format!("échec de construction du graphe: {err}"),
            )
        })?;

        build_snapshot(&gps, &graph, SnapshotOptions::new(config.filename.clone())).await?
    };

    eprintln!(
        "viewer loaded: {} segments, {} interests",
        snapshot_data.metrics.segments, snapshot_data.metrics.interests
    );

    eprintln!("static files: {}", config.static_dir.display());

    let bench = BenchStore::load(env!("CARGO_MANIFEST_DIR"))?;

    let state = AppState {
        gps: Arc::new(gps),
        order: config.order,
        snapshot: Arc::new(snapshot_data),
        bench: Arc::new(bench),
    };

    let app = Router::new()
        .route("/api/snapshot", get(snapshot))
        .route("/api/snap", get(snap))
        .route("/api/bench/config", get(bench_config))
        .route("/api/bench/result", get(bench_result))
        .route("/api/visualize/result", get(visualize_result))
        .route("/api/visualize/events", get(visualize_events))
        .fallback_service(ServeDir::new(&config.static_dir))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    eprintln!(
        "Block size: {}, RAM #Blocks: {}, FLASH #Blocks: {}",
        swap_cache::config::BLOCK_SIZE,
        CAPACITY,
        FLASH_SIZE
    );

    eprintln!("viewer: http://{}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

#[derive(Debug)]
struct ViewerConfig {
    gps_path: PathBuf,
    static_dir: PathBuf,
    filename: String,
    port: u16,
    order: TileOrder,
}

impl ViewerConfig {
    fn from_args() -> IoResult<Self> {
        let args: Vec<String> = env::args().collect();

        let filename = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| "grenoble_large.gps".to_string());

        let port = args
            .get(2)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(3000);

        let order = match args.get(3).map(String::as_str) {
            Some("normal") | Some("row-major") => TileOrder::RowMajor,
            Some("hilbert") => TileOrder::Hilbert,
            Some("zcurve") | Some("z-curve") => TileOrder::Morton,

            Some(value) => {
                return Err(IoError::new(
                    ErrorKind::InvalidInput,
                    format!("layout inconnu: {value}"),
                ));
            }

            None if filename.contains("hilbert") => TileOrder::Hilbert,
            None if filename.contains("zcurve") => TileOrder::Morton,
            None => TileOrder::RowMajor,
        };

        let mut gps_path = env::current_dir()?;
        gps_path.push("data");
        gps_path.push(&filename);

        let mut static_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        static_dir.push("web");
        static_dir.push("viewer");

        Ok(Self {
            gps_path,
            static_dir,
            filename,
            port,
            order,
        })
    }
}

fn bad_request(err: std::io::Error) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err.to_string())
}
