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

use crate::{
	prelude::*,
	set_boot_time, 
	tasks::{
		i2c::I2cCommand,
		charter::CharterState,
		shutdown::ShutdownRequest
	}
};

use esp_idf_svc::{
	http::server::{
		EspHttpConnection,
		EspHttpServer
	},
	io::EspIOError,
	hal::task::block_on
};

use embedded_svc::{
	http::server::{
		CompositeHandler, Handler, Middleware, Connection
	},
	http::{
		Headers, Method
	},
	io::{
		Read
	}
};
use crate::tasks::stepper_controller::UartRelease;
////////////////////////////////////////////////////////////////////////////////

pub fn initialize_http_server<'server>(
	shutdown_signal_sender: &'static ShutdownSignal,
	i2c_sender: I2cSender<'static>,
	charter_state_sender: CharterStateSender<'static>,
	uart_release_sender: UartReleaseSender<'static>,
) -> Result<EspHttpServer<'server>, EspIOError> {
	let mut server = EspHttpServer::new(&Default::default())?;

	server
		.handler("/heartbeat",    Method::Get,  GetHeartbeat)?
		.handler("/status",       Method::Get,  pep(GetStatus { i2c_sender }))?
		.handler("/config",       Method::Post, pep(PostConfig {}))?
		.handler("/shutdown",     Method::Post, pep(PostShutdown { shutdown_signal_sender }))?
		.handler("/start_dive",   Method::Post, pep(PostStartDive { charter_state_sender }))?
		.handler("/time_sync",    Method::Post, pep(PostTimeSync))?
		.handler("/release_uart", Method::Post, pep(PostReleaseUart { uart_release_sender }))?
	;

	Ok(server)
}

////////////////////////////////////////////////////////////////////////////////

// TODO: Maybe make a macro that generates these structs?

/// Noop req. We don't use PlainErrorPage to make handling the connection faster.
struct GetHeartbeat;
impl<'request> Handler<EspHttpConnection<'request>> for GetHeartbeat {
	type Error = AnyhowError;

	fn handle(&self, conn: &mut EspHttpConnection) -> Result<(), AnyhowError> {
		reply_204(conn)
	}
}

struct GetStatus { i2c_sender: I2cSender<'static> }
impl<'request> Handler<EspHttpConnection<'request>> for GetStatus {
	type Error = AnyhowError;

	fn handle(&self, conn: &mut EspHttpConnection) -> Result<(), AnyhowError> {
		info!("Handling request");

		let (response, receiver) = futures::channel::oneshot::channel();
		if self.i2c_sender.try_send(I2cCommand::GetTH {
			response
		}).is_err() {
			return Err(AnyhowError::msg("I2C channel is full"));
		}

		info!("Sent i2c req");

		let th = block_on(receiver)?;

		info!("Received {th:?}");

		reply(conn, 200, format!("Temp: {} C, Humidity: {}%\n", th.celsius, th.relative_humidity))
	}
}

struct PostTimeSync;
impl<'request> Handler<EspHttpConnection<'request>> for PostTimeSync {
	type Error = AnyhowError;

	fn handle(&self, conn: &mut EspHttpConnection) -> Result<(), AnyhowError> {
		// The control station will have to account for connection latency
		set_boot_time(serde_json::from_slice(read_body(conn)?.as_slice())?);

		reply_204(conn)
	}
}

// TODO: figure out what the `'request` lifetime actually represents: request or handler?
struct PostConfig { }
impl<'request> Handler<EspHttpConnection<'request>> for PostConfig {
	type Error = AnyhowError;

	fn handle(&self, conn: &mut EspHttpConnection) -> Result<(), AnyhowError> {
		//! TODO
		reply_204(conn)
	}
}

struct PostStartDive { charter_state_sender: CharterStateSender<'static> }
impl<'request> Handler<EspHttpConnection<'request>> for PostStartDive {
	type Error = AnyhowError;

	fn handle(&self, conn: &mut EspHttpConnection) -> Result<(), AnyhowError> {
		info!("Got request to POST /start_dive");

		self.charter_state_sender.send(CharterState::StartRequested);

		reply_204(conn)
	}
}

struct PostShutdown { shutdown_signal_sender: &'static ShutdownSignal }
impl<'request> Handler<EspHttpConnection<'request>> for PostShutdown {
	type Error = AnyhowError;

	fn handle(&self, conn: &mut EspHttpConnection) -> Result<(), AnyhowError> {
		self.shutdown_signal_sender.signal(ShutdownRequest { originator: "http request", go_to_surface: true });

		reply_204(conn)
	}
}

struct PostReleaseUart { uart_release_sender: UartReleaseSender<'static> }
impl<'request> Handler<EspHttpConnection<'request>> for PostReleaseUart {
	type Error = AnyhowError;

	fn handle(&self, conn: &mut EspHttpConnection) -> Result<(), AnyhowError> {
		self.uart_release_sender.send(UartRelease::Requested);

		reply_204(conn)
	}
}

////////////////////////////////////////////////////////////////////////////////

fn read_body(conn: &mut EspHttpConnection) -> Result<Vec<u8>, AnyhowError> {
	// Construct a buffer with the exact size of the request data
	let mut buf = vec![0; conn.content_len().unwrap_or(0) as usize];
	match conn.read_exact(&mut buf) {
		Err(error) => Err(AnyhowError::from(error)),
		_ => Ok(buf),
	}
}

fn reply_204(conn: &mut EspHttpConnection) -> Result<(), AnyhowError> {
	conn.initiate_response(204, None, &[])
		.map_err(AnyhowError::from)
}

fn reply(conn: &mut EspHttpConnection, code: u16, content: String) -> Result<(), AnyhowError> {
	conn.initiate_response(code, None, &[("Content-Type", "text/plain; charset=utf-8")])?;

	conn.write_all(content.as_bytes())?;

	Ok(())
}

/// The default error page is a bunch of unnecessary HTML; make it better
struct PlainErrorPage;

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
