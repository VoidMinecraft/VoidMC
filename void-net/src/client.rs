struct Client {
    socket: ClientSocket,
}

impl Client {
    pub fn new(socket: ClientSocket) -> Self {
        Self { socket }
    }

    pub async fn run(&mut self) -> std::io::Result<()> {
        // Client logic goes here
        Ok(())
    }
}
