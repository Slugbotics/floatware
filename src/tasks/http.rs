//! This isn't technically a task (in the sense that we mean a looping async
//! function called by [floatware::initialize]), but I put it in the tasks
//! directory because it, like its siblings, is a file full of code with similar
//! goals: setting up and handling requests for the onboard http server.
//!
//! Each individual path and method gets its own handler that is initialized owning
//! a reference to some kind of communication channel, which it uses to execute its
//! goals. For (hopefully) readability, I've named each handler after the path it
//! patrols, e.g. [GetStatus] handles `GET /status`.
//!
//! Very simple handlers are implemented as plain functions using
//! [EspHttpServer::fn_handler]. There may not be any such handlers at the time you
//! read this.
//!
//! TODO: once I am done testing things with JSON typed into curl, port to MessagePack

use crate::{tasks::led::_SerdeRGB8, signals::{LedSignal, ShutdownSignal}};

use std::sync::Arc;

use esp_idf_svc::{
	http::server::{
		Configuration as HttpServerConfiguration,
		EspHttpConnection,
		EspHttpServer
	},
	io::EspIOError
};

use embedded_svc::{
	http::server::{
		CompositeHandler, Handler, Middleware, Request
	},
	http::{
		Headers, Method
	},
	io::{
		Read, Write
	}
};

use anyhow::Error as AnyhowError;
use embedded_svc::http::server::Connection;
use log::{error, info};

////////////////////////////////////////////////////////////////////////////////

pub fn initialize_http_server<'server>(
	led_signal: Arc<LedSignal>,
	shutdown_signal: Arc<ShutdownSignal>,
) -> Result<EspHttpServer<'server>, EspIOError> {
	let mut server = EspHttpServer::new(&HttpServerConfiguration {
		..Default::default()
	})?;

	server.fn_handler("/status",   Method::Get,  http_handle_status)?;
	server.fn_handler("/config",   Method::Post, http_handle_config)?;
	server.   handler("/shutdown", Method::Post, pep(PostShutdown { shutdown_signal }))?;
	server.   handler("/led",      Method::Post, pep(PostLed { led_signal }))?;

	Ok(server)
}

////////////////////////////////////////////////////////////////////////////////

fn http_handle_status(req: Request<&mut EspHttpConnection>) -> Result<(), EspIOError> {
	//! TODO
	req.into_ok_response()?
		.write_all("Hello, client!\n".as_bytes())
}

fn http_handle_config(req: Request<&mut EspHttpConnection>) -> Result<(), AnyhowError> {
	//! TODO
	req.into_status_response(204)?;
	Ok(())
}

struct PostShutdown { shutdown_signal: Arc<ShutdownSignal> }
impl<'request> Handler<EspHttpConnection<'request>> for PostShutdown {
	type Error = AnyhowError;

	fn handle(&self, conn: &mut EspHttpConnection) -> Result<(), AnyhowError> {
		self.shutdown_signal.signal(());

		conn.initiate_response(204, None, &[])
			.map_err(AnyhowError::from)
	}
}

struct PostLed { led_signal: Arc<LedSignal> }
impl<'request> Handler<EspHttpConnection<'request>> for PostLed {
	type Error = AnyhowError;

	fn handle(&self, conn: &mut EspHttpConnection) -> Result<(), AnyhowError> {
		let buf = read_body(conn)?;

		let json = str::from_utf8(buf.as_slice())?;

		info!("Got request: {json}");

		self.led_signal.signal(
			_SerdeRGB8::deserialize(
				&mut serde_json::Deserializer::from_str(
					&json
				)
			)?
		);

		conn.initiate_response(204, None, &[])
			.map_err(AnyhowError::from)
	}
}

fn read_body(conn: &mut EspHttpConnection) -> Result<Vec<u8>, AnyhowError> {
	// Construct a buffer with the exact size of the request data
	let mut buf = vec![0; conn.content_len().unwrap_or(0) as usize];
	match conn.read_exact(&mut buf) {
		Err(error) => Err(AnyhowError::from(error)),
		_ => Ok(buf),
	}
}

////////////////////////////////////////////////////////////////////////////////

/// The default error page is a bunch of unnecessary HTML; make it better
struct PlainErrorPage /* : Middleware */;

impl<'request, H: Handler<EspHttpConnection<'request>, Error = AnyhowError>>
Middleware<EspHttpConnection<'request>, H> for PlainErrorPage {
	type Error = AnyhowError;

	/// On error, return a plain error page with the error, if possible
	fn handle(&self, conn: &mut EspHttpConnection<'request>, handler: &H) -> Result<(), Self::Error> {
		info!("Handling {:?} request to {}", conn.method(), conn.uri());
		if let Err(error) = handler.handle(conn) {
			error!("Error encountered: {error:#}");

			if conn.is_response_initiated() {
				return Err(error); // too late to act
			}

			conn.initiate_response(500, Some("Internal Server Error"), &[
				("Content-Type", "text/plain; charset=utf-8")
			])?;

			conn.write_all(
				format!("Error encountered processing request:\n\n{:#}\n", error).as_bytes()
			)?;
		}

		Ok(())
	}
}

/// Convenience function to shorten usage of [PlainErrorPage]
fn pep<
	'request, H : Handler<EspHttpConnection<'request>, Error = AnyhowError>
>(handler: H) -> CompositeHandler<PlainErrorPage, H> {
	CompositeHandler::new(PlainErrorPage, handler)
}
