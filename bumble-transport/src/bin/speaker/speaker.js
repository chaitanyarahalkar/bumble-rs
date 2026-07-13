(() => {
  'use strict';
  const $ = id => document.getElementById(id);
  const channelUrl = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/channel`;
  let socket, codec, decoder, context, analyser, audioEnabled = false;
  let packets = 0, bytes = 0, samples = [], bins = [], nextAudioTime = 0;

  function setStreamState(state) { $('streamStateText').textContent = state; }
  function alert(message) {
    $('connectionText').textContent = message;
    $('connectionText').style.display = message ? 'block' : 'none';
  }

  function configureDecoder() {
    if (!audioEnabled || decoder || !window.AudioDecoder) return;
    if (codec !== 'aac' && codec !== 'opus') return;
    decoder = new AudioDecoder({
      output: audio => {
        const buffer = context.createBuffer(audio.numberOfChannels, audio.numberOfFrames, audio.sampleRate);
        for (let channel = 0; channel < audio.numberOfChannels; channel++) {
          audio.copyTo(buffer.getChannelData(channel), { planeIndex: channel, format: 'f32-planar' });
        }
        const source = context.createBufferSource();
        source.buffer = buffer;
        source.connect(analyser);
        nextAudioTime = Math.max(nextAudioTime, context.currentTime);
        source.start(nextAudioTime);
        nextAudioTime += audio.duration / 1_000_000;
        audio.close();
      },
      error: error => console.error('audio decoder error', error)
    });
    decoder.configure(codec === 'aac'
      ? { codec: 'mp4a.40.2', sampleRate: 44100, numberOfChannels: 2 }
      : { codec: 'opus', sampleRate: 48000, numberOfChannels: 2 });
  }

  function audioPacket(data) {
    packets += 1; bytes += data.byteLength;
    $('packetsReceivedText').textContent = packets.toLocaleString();
    $('bytesReceivedText').textContent = bytes.toLocaleString();
    bins.push(data.byteLength); if (bins.length > 200) bins.shift();
    samples.push({ time: performance.now(), bytes: data.byteLength });
    if (samples.length > 40) samples.shift();
    if (samples.length > 1) {
      const elapsed = samples.at(-1).time - samples[0].time;
      const windowBytes = samples.slice(1).reduce((total, sample) => total + sample.bytes, 0);
      $('bitrate').textContent = `${Math.round(8 * windowBytes / elapsed)} kb/s`;
    }
    configureDecoder();
    if (audioEnabled && decoder) {
      decoder.decode(new EncodedAudioChunk({ type: 'key', data, timestamp: 0 }));
    }
  }

  const handlers = {
    hello: params => {
      codec = params.codec;
      $('codecText').textContent = codec.toUpperCase();
      setStreamState(params.streamState || 'IDLE');
      if (codec === 'sbc') {
        $('audioOnButton').disabled = true;
        $('audioSupportMessageText').textContent = 'Browser playback supports AAC and Opus; monitoring remains active.';
      }
    },
    connection: params => { $('connectionStateText').textContent = `CONNECTED: ${params.peer_name || 'unknown'} (${params.peer_address})`; },
    disconnection: () => { $('connectionStateText').textContent = 'DISCONNECTED'; },
    start: () => setStreamState('STARTED'),
    stop: () => setStreamState('STOPPED'),
    suspend: () => setStreamState('SUSPENDED')
  };

  function connect() {
    socket = new WebSocket(channelUrl);
    socket.binaryType = 'arraybuffer';
    socket.onopen = () => { alert(''); socket.send(JSON.stringify({ type: 'hello' })); };
    socket.onclose = () => { alert('Connection to the speaker process closed.'); setTimeout(connect, 1500); };
    socket.onerror = () => alert('Unable to connect to the speaker process.');
    socket.onmessage = event => {
      if (typeof event.data === 'string') {
        const message = JSON.parse(event.data);
        handlers[message.type]?.(message.params || {});
      } else {
        audioPacket(event.data);
      }
    };
  }

  function draw() {
    const bandwidth = $('bandwidthCanvas'), bctx = bandwidth.getContext('2d');
    bctx.clearRect(0, 0, bandwidth.width, bandwidth.height);
    bctx.fillStyle = '#e7aa2b';
    const peak = Math.max(1, ...bins);
    bins.forEach((value, index) => bctx.fillRect(index * 4, bandwidth.height, 3, -value / peak * bandwidth.height));
    const fft = $('fftCanvas'), fctx = fft.getContext('2d');
    fctx.clearRect(0, 0, fft.width, fft.height);
    if (analyser) {
      const frequencies = new Uint8Array(analyser.frequencyBinCount);
      analyser.getByteFrequencyData(frequencies);
      fctx.fillStyle = '#5aa7e8';
      frequencies.forEach((value, index) => fctx.fillRect(index * 12, fft.height, 10, -value / 255 * fft.height));
    }
    requestAnimationFrame(draw);
  }

  window.addEventListener('DOMContentLoaded', () => {
    context = new AudioContext();
    analyser = context.createAnalyser(); analyser.fftSize = 128; analyser.connect(context.destination);
    $('audioOnButton').onclick = async () => {
      await context.resume(); audioEnabled = true; $('audioOnButton').disabled = true; configureDecoder();
    };
    connect(); draw();
  });
})();
