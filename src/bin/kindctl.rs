use clap::{Parser, Subcommand};
use tonic::transport::Channel;

pub mod kind_pb {
    tonic::include_proto!("kind");
}

use kind_pb::kind_service_client::KindServiceClient;
use kind_pb::{
    CasRequest, DeleteRequest, GetRequest, PutRequest, QueryRequest, RangeScanRequest,
};

#[derive(Parser)]
#[command(
    name = "kindctl",
    about = "CLI client for Kind DB",
    version = "0.1.0",
    long_about = None
)]
struct Cli {
    #[arg(long, default_value = "localhost:50051", global = true)]
    host: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Store a key-value pair
    Put {
        key: String,
        value: String,
        /// Time-to-live in milliseconds (omit for no expiry)
        #[arg(long)]
        ttl: Option<u64>,
    },
    /// Retrieve the value for a key
    Get {
        key: String,
    },
    /// Delete a key
    Del {
        key: String,
    },
    /// Scan all keys in an inclusive [lo, hi] range
    Scan {
        lo: String,
        hi: String,
    },
    /// Query records by a secondary index field (must be @indexed in schema.ksl)
    Query {
        schema_type: String,
        field: String,
        value: String,
        /// Maximum number of results [default: 100]
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Number of results to skip for pagination [default: 0]
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// Atomically update a key only if the current value matches expected
    Cas {
        key: String,
        expected: String,
        new_value: String,
        /// Time-to-live for the new value in milliseconds
        #[arg(long)]
        ttl: Option<u64>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let endpoint = format!("http://{}", cli.host);
    let channel = Channel::from_shared(endpoint)?
        .connect()
        .await
        .map_err(|e| {
            eprintln!("Could not connect to Kind DB at '{}': {}", cli.host, e);
            eprintln!("Is the server running? Try: docker run -d -p 50051:50051 nks01x/kind-db:latest");
            e
        })?;

    let mut client = KindServiceClient::new(channel);

    match cli.command {
        Commands::Put { key, value, ttl } => {
            let request = tonic::Request::new(PutRequest {
                key: key.clone(),
                value: value.as_bytes().to_vec(),
                ttl_ms: ttl,
            });
            let response = client.put(request).await?;
            if response.into_inner().success {
                println!("stored \"{}\"", key);
            } else {
                eprintln!("failed to store \"{}\"", key);
                std::process::exit(1);
            }
        }

        Commands::Get { key } => {
            let request = tonic::Request::new(GetRequest { key: key.clone() });
            match client.get(request).await {
                Ok(response) => {
                    let record = response.into_inner();
                    let value = String::from_utf8_lossy(&record.value);
                    match serde_json::from_str::<serde_json::Value>(&value) {
                        Ok(json) => println!("{}", serde_json::to_string_pretty(&json)?),
                        Err(_) => println!("{}", value),
                    }
                    if let Some(exp) = record.expires_at {
                        println!("\nexpires at: {} ms (unix epoch)", exp);
                    }
                }
                Err(status) if status.code() == tonic::Code::NotFound => {
                    eprintln!("key not found: \"{}\"", key);
                    std::process::exit(1);
                }
                Err(e) => return Err(e.into()),
            }
        }

        Commands::Del { key } => {
            let request = tonic::Request::new(DeleteRequest { key: key.clone() });
            let response = client.delete(request).await?;
            if response.into_inner().success {
                println!("deleted \"{}\"", key);
            } else {
                eprintln!("key not found: \"{}\"", key);
                std::process::exit(1);
            }
        }

        Commands::Scan { lo, hi } => {
            let request = tonic::Request::new(RangeScanRequest {
                lo: lo.clone(),
                hi: hi.clone(),
            });
            let response = client.range_scan(request).await?;
            let records = response.into_inner().records;

            if records.is_empty() {
                println!("no records found in range \"{}\" ..= \"{}\"", lo, hi);
                return Ok(());
            }

            println!("{} record(s) in [\"{}\" ..= \"{}\"]:\n", records.len(), lo, hi);
            for rec in records {
                let value = String::from_utf8_lossy(&rec.value);
                println!("key: {}", rec.key);
                match serde_json::from_str::<serde_json::Value>(&value) {
                    Ok(json) => println!("val: {}", serde_json::to_string_pretty(&json)?),
                    Err(_) => println!("val: {}", value),
                }
                println!();
            }
        }

        Commands::Query { schema_type, field, value, limit, offset } => {
            let request = tonic::Request::new(QueryRequest {
                schema_type: schema_type.clone(),
                field: field.clone(),
                value: value.clone(),
                limit: Some(limit),
                offset: Some(offset),
            });
            let response = client.query(request).await?;
            let records = response.into_inner().records;

            if records.is_empty() {
                println!("no records found where {}.{} = \"{}\"", schema_type, field, value);
                return Ok(());
            }

            println!("{} record(s) where {}.{} = \"{}\":\n", records.len(), schema_type, field, value);
            for rec in records {
                let value_str = String::from_utf8_lossy(&rec.value);
                println!("key: {}", rec.key);
                match serde_json::from_str::<serde_json::Value>(&value_str) {
                    Ok(json) => println!("val: {}", serde_json::to_string_pretty(&json)?),
                    Err(_) => println!("val: {}", value_str),
                }
                println!();
            }
        }

        Commands::Cas { key, expected, new_value, ttl } => {
            let request = tonic::Request::new(CasRequest {
                key: key.clone(),
                expected_value: expected.as_bytes().to_vec(),
                new_value: new_value.as_bytes().to_vec(),
                ttl_ms: ttl,
            });
            let response = client.cas(request).await?;
            if response.into_inner().success {
                println!("cas succeeded: \"{}\" updated", key);
            } else {
                eprintln!("cas failed: current value of \"{}\" did not match expected", key);
                eprintln!("hint: run `kindctl get {}` to see the current value", key);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
