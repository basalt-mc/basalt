use super::*;
use basalt_protocol::packets::status::{
    ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPing,
    ServerboundStatusPingStart,
};

#[tokio::test]
async fn e2e_server_status_ping() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    // Handshake → Status
    client_handshake(&mut client, addr.port(), 1).await;

    // StatusRequest
    send_packet(
        &mut client,
        ServerboundStatusPingStart::PACKET_ID,
        &ServerboundStatusPingStart,
    )
    .await;

    // Read StatusResponse
    let (id, response): (_, ClientboundStatusServerInfo) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundStatusServerInfo::PACKET_ID);
    assert!(response.response.contains("Basalt"));
    assert!(response.response.contains("769"));

    // Ping/Pong
    send_packet(
        &mut client,
        ServerboundStatusPing::PACKET_ID,
        &ServerboundStatusPing { time: 123 },
    )
    .await;

    let (id, pong): (_, ClientboundStatusPing) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundStatusPing::PACKET_ID);
    assert_eq!(pong.time, 123);
}
