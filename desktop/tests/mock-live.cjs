// Isolated fixture: no Sonar environment, database, or real account is used.
const http = require("node:http");
const { Server } = require("../../backend/node_modules/socket.io");
const server = http.createServer();
const io = new Server(server, { transports: ["websocket"] });
io.use((socket, next) => socket.handshake.headers.cookie === "jwt=synthetic-live-session" ? next() : next(new Error("unauthorized")));
io.on("connection", socket => {
  setTimeout(() => socket.emit("getOnlineUsers", ["0123456789abcdef01234567"]), 50);
  setTimeout(() => socket.emit("matrixRoomChanged"), 75);
  setTimeout(() => socket.emit("sessionRevoked"), 100);
});
server.listen(0, "127.0.0.1", () => process.stdout.write(`${server.address().port}\n`));
