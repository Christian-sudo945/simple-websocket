let ws;
let localStream;
let peerConnections = {};
let voiceRoomId = null;
const configuration = { 
    iceServers: [
        { urls: 'stun:stun.l.google.com:19302' },
        { urls: 'stun:stun1.l.google.com:19302' }
    ]
};

// WebSocket Connection
function connectWebSocket() {
    ws = new WebSocket('ws://localhost:8080');

    ws.onopen = () => {
        console.log('Connected to server');
        document.getElementById('sendMessage').disabled = false;
    };

    ws.onmessage = async (event) => {
        const data = JSON.parse(event.data);
        
        switch(data.type) {
            case 'chat':
                addMessage(data.userId, data.message, data.userId === 'You');
                break;
            case 'userList':
                updateUserList(data.users);
                break;
            case 'offer':
                await handleOffer(data);
                break;
            case 'answer':
                await handleAnswer(data);
                break;
            case 'ice-candidate':
                await handleNewICECandidate(data);
                break;
            case 'join-voice':
                handleUserJoinVoice(data.userId, data.roomId);
                break;
            case 'leave-voice':
                handleUserLeaveVoice(data.userId);
                break;
        }
    };

    ws.onclose = () => {
        console.log('Disconnected from server');
        setTimeout(connectWebSocket, 1000);
    };
}

function addMessage(userId, message, isSelf) {
    const messagesDiv = document.getElementById('chatMessages');
    const messageElement = document.createElement('div');
    messageElement.className = `message ${isSelf ? 'sent' : 'received'}`;
    messageElement.textContent = `${isSelf ? 'You' : 'User ' + userId}: ${message}`;
    messagesDiv.appendChild(messageElement);
    messagesDiv.scrollTop = messagesDiv.scrollHeight;
}

function updateUserList(users) {
    const userList = document.getElementById('userList');
    userList.innerHTML = '';
    users.forEach(user => {
        const userElement = document.createElement('div');
        userElement.className = 'user-item';
        userElement.innerHTML = `
            <div class="status-indicator"></div>
            <span>User ${user}</span>
            <button onclick="inviteToVoice(${user})">Invite to Voice</button>
        `;
        userList.appendChild(userElement);
    });
}

async function startVoiceCall(roomId) {
    try {
        localStream = await navigator.mediaDevices.getUserMedia({ audio: true });
        voiceRoomId = roomId || Date.now().toString();
        
        ws.send(JSON.stringify({
            type: 'join-voice',
            roomId: voiceRoomId
        }));

        document.getElementById('startVoice').disabled = true;
        document.getElementById('endVoice').disabled = false;
    } catch (error) {
        console.error('Error accessing microphone:', error);
    }
}

function inviteToVoice(userId) {
    ws.send(JSON.stringify({
        type: 'voice-invite',
        targetUserId: userId,
        roomId: voiceRoomId || Date.now().toString()
    }));
}

function endVoiceCall() {
    if (localStream) {
        localStream.getTracks().forEach(track => track.stop());
        localStream = null;
    }

    Object.values(peerConnections).forEach(conn => conn.close());
    peerConnections = {};

    ws.send(JSON.stringify({
        type: 'leave-voice',
        roomId: voiceRoomId
    }));

    voiceRoomId = null;
    document.getElementById('startVoice').disabled = false;
    document.getElementById('endVoice').disabled = true;
}

async function createPeerConnection(userId) {
    if (!peerConnections[userId]) {
        const peerConnection = new RTCPeerConnection(configuration);
        peerConnections[userId] = peerConnection;

        if (localStream) {
            localStream.getTracks().forEach(track => {
                peerConnection.addTrack(track, localStream);
            });
        }

        peerConnection.onicecandidate = (event) => {
            if (event.candidate) {
                ws.send(JSON.stringify({
                    type: 'ice-candidate',
                    candidate: event.candidate,
                    targetUserId: userId,
                    roomId: voiceRoomId
                }));
            }
        };

        peerConnection.ontrack = (event) => {
            const audio = new Audio();
            audio.srcObject = event.streams[0];
            audio.play();
        };

        const offer = await peerConnection.createOffer();
        await peerConnection.setLocalDescription(offer);
        ws.send(JSON.stringify({
            type: 'offer',
            offer: offer,
            targetUserId: userId,
            roomId: voiceRoomId
        }));
    }
    return peerConnections[userId];
}

async function handleOffer(data) {
    if (!peerConnections[data.userId]) {
        const peerConnection = new RTCPeerConnection(configuration);
        peerConnections[data.userId] = peerConnection;

        if (localStream) {
            localStream.getTracks().forEach(track => {
                peerConnection.addTrack(track, localStream);
            });
        }

        peerConnection.onicecandidate = (event) => {
            if (event.candidate) {
                ws.send(JSON.stringify({
                    type: 'ice-candidate',
                    candidate: event.candidate,
                    targetUserId: data.userId,
                    roomId: voiceRoomId
                }));
            }
        };

        peerConnection.ontrack = (event) => {
            const audio = new Audio();
            audio.srcObject = event.streams[0];
            audio.play();
        };
    }

    const peerConnection = peerConnections[data.userId];
    await peerConnection.setRemoteDescription(new RTCSessionDescription(data.offer));
    const answer = await peerConnection.createAnswer();
    await peerConnection.setLocalDescription(answer);

    ws.send(JSON.stringify({
        type: 'answer',
        answer: answer,
        targetUserId: data.userId,
        roomId: voiceRoomId
    }));
}

async function handleAnswer(data) {
    const peerConnection = peerConnections[data.userId];
    if (peerConnection) {
        await peerConnection.setRemoteDescription(new RTCSessionDescription(data.answer));
    }
}

async function handleNewICECandidate(data) {
    const peerConnection = peerConnections[data.userId];
    if (peerConnection) {
        try {
            await peerConnection.addIceCandidate(new RTCIceCandidate(data.candidate));
        } catch (error) {
            console.error('Error adding ICE candidate:', error);
        }
    }
}

function handleUserJoinVoice(userId, roomId) {
    if (roomId === voiceRoomId) {
        createPeerConnection(userId);
    }
}

function handleUserLeaveVoice(userId) {
    if (peerConnections[userId]) {
        peerConnections[userId].close();
        delete peerConnections[userId];
    }
}

// Event Listeners
document.getElementById('sendMessage').addEventListener('click', () => {
    const input = document.getElementById('messageInput');
    const message = input.value.trim();
    if (message) {
        ws.send(JSON.stringify({ 
            type: 'chat', 
            message: message 
        }));
        input.value = '';
    }
});

document.getElementById('messageInput').addEventListener('keypress', (e) => {
    if (e.key === 'Enter') {
        document.getElementById('sendMessage').click();
    }
});

document.getElementById('startVoice').addEventListener('click', () => startVoiceCall());
document.getElementById('endVoice').addEventListener('click', endVoiceCall);

// Initialize
connectWebSocket();