use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};
use async_std::task;
use std::env;
use tide::Request;
use tide::{http::mime, Body, Response, StatusCode};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct MyToken(String);

struct QueryRoot;

#[Object]
impl QueryRoot {
  async fn current_token<'a>(&self, ctx: &'a Context<'_>) -> Option<&'a str> {
    ctx.data_opt::<MyToken>().map(|token| token.0.as_str())
  }
}

#[derive(Clone)]
struct AppState {
  schema: Schema<QueryRoot, EmptyMutation, EmptySubscription>,
}

fn main() -> Result<()> {
  task::block_on(run())
}

async fn run() -> Result<()> {
  let listen_addr = env::var("LISTEN_ADDR").unwrap_or_else(|_| "localhost:8000".to_owned());
  let schema = Schema::build(QueryRoot, EmptyMutation, EmptySubscription).finish();

  println!("Playground: http://{}", listen_addr);

  let mut app = tide::new();

  app.at("/graphql").post(move |req: Request<()>| {
    let schema = schema.clone();
    async move {
      let token = req.header("token").and_then(|values| values.get(0)).map(|value| value.as_str().to_string());

      let mut req = async_graphql_tide::receive_request(req).await?;
      if let Some(token) = token {
        req = req.data(MyToken(token));
      }
      async_graphql_tide::respond(schema.execute(req).await)
    }
  });

  app.at("/").get(|_| async move {
    let mut resp = Response::new(StatusCode::Ok);
    resp.set_body(Body::from_string(playground_source(GraphQLPlaygroundConfig::new("/graphql"))));
    resp.set_content_type(mime::HTML);
    Ok(resp)
  });

  app.listen(listen_addr).await?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use async_std::prelude::*;
  use serde_json::json;
  use std::time::Duration;

  #[test]
  fn sample() -> Result<()> {
    task::block_on(async {
      let listen_addr = find_listen_addr().await;
      env::set_var("LISTEN_ADDR", format!("{}", listen_addr));

      let server: task::JoinHandle<Result<()>> = task::spawn(async move {
        run().await?;

        Ok(())
      });

      let client: task::JoinHandle<Result<()>> = task::spawn(async move {
        let listen_addr = env::var("LISTEN_ADDR").unwrap();

        task::sleep(Duration::from_millis(300)).await;

        let string = surf::post(format!("http://{}/graphql", listen_addr))
          .body(Body::from(r#"{"query":"{ currentToken }"}"#))
          .header("Content-Type", "application/json")
          .header("Token", "1234")
          .recv_string()
          .await?;

        assert_eq!(string, json!({"data":{"currentToken":"1234"}}).to_string());

        let string = surf::post(format!("http://{}/graphql", listen_addr))
          .body(Body::from(r#"{"query":"{ currentToken }"}"#))
          .header("Content-Type", "application/json")
          .recv_string()
          .await?;

        assert_eq!(string, json!({"data":{"currentToken":null}}).to_string());

        Ok(())
      });

      server.race(client).await?;

      Ok(())
    })
  }

  async fn find_listen_addr() -> async_std::net::SocketAddr {
    async_std::net::TcpListener::bind("localhost:0").await.unwrap().local_addr().unwrap()
  }
}
