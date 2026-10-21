// The fantasy sky: a gradient that runs from day to dusk to night, the sun (or the moon at night), stars, and a
// layer of drifting clouds whose cover and darkness the weather sets. Driven from theme.gd every frame.
shader_type sky;

uniform vec3 sun_dir = vec3(0.4, 0.6, 0.3);   // towards the sun; the moon is opposite
uniform float day = 1.0;                       // 1 full day, 0 full night
uniform float dusk = 0.0;                      // 1 when the sun is on the horizon
uniform float cover = 0.25;                    // cloud cover, 0 clear .. 1 overcast
uniform float dark = 0.0;                      // how much the weather darkens everything
uniform vec3 tint = vec3(0.55, 0.6, 0.66);     // the weather's colour, for overcast cloud
uniform float flash = 0.0;                     // lightning, 0 .. 1
uniform float clock = 0.0;                     // seconds, for everything that moves
uniform float wind = 0.3;                      // how fast the clouds go: a breeze when clear, racing in a storm

const vec3 TOP_DAY = vec3(0.16, 0.36, 0.72);
const vec3 HORIZON_DAY = vec3(0.6, 0.74, 0.86);
const vec3 TOP_NIGHT = vec3(0.012, 0.02, 0.05);
const vec3 HORIZON_NIGHT = vec3(0.05, 0.07, 0.12);
const vec3 DUSK = vec3(1.0, 0.48, 0.22);
const vec3 GROUND = vec3(0.17, 0.21, 0.14);

float hash(vec2 p) {
	p = fract(p * vec2(123.34, 456.21));
	p += dot(p, p + 45.32);
	return fract(p.x * p.y);
}

float noise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

float fbm(vec2 p) {
	float v = 0.0;
	float a = 0.5;
	for (int i = 0; i < 5; i++) {
		v += a * noise(p);
		p = p * 2.03 + vec2(17.1, 3.7);
		a *= 0.5;
	}
	return v;
}

void sky() {
	vec3 d = EYEDIR;
	float up = clamp(d.y, 0.0, 1.0);
	vec3 top = mix(TOP_NIGHT, TOP_DAY, day);
	vec3 horizon = mix(HORIZON_NIGHT, HORIZON_DAY, day);
	// Dusk warms the horizon, most towards the sun.
	float toward = pow(max(dot(normalize(vec3(d.x, 0.0, d.z)), normalize(vec3(sun_dir.x, 0.0, sun_dir.z))), 0.0), 3.0);
	horizon = mix(horizon, DUSK, dusk * (0.35 + 0.65 * toward));
	vec3 col = mix(horizon, top, pow(up, 0.55));
	if (d.y < 0.0) {
		col = mix(horizon, GROUND * (0.25 + 0.75 * day), clamp(-d.y * 5.0, 0.0, 1.0));
	}
	// Stars, fading out with the day and behind any cloud, each twinkling at its own pace.
	if (d.y > 0.0) {
		vec2 sp = floor(d.xz / (d.y + 0.35) * 260.0);
		float h = hash(sp + 7.0);
		float twinkle = 0.55 + 0.45 * sin(clock * (1.5 + 3.0 * h) + h * 40.0);
		float s = step(0.9975, hash(sp)) * (1.0 - day) * smoothstep(0.0, 0.25, d.y);
		col += vec3(s) * (0.5 + 0.5 * h) * twinkle;
		// Now and then a shooting star: one streak per 9-second slot, in a different place each time.
		float slot = floor(clock / 9.0);
		float age = fract(clock / 9.0) * 9.0;
		if (age < 0.9 && hash(vec2(slot, 3.1)) > 0.35) {
			vec2 from = vec2(hash(vec2(slot, 1.7)) * 2.0 - 1.0, hash(vec2(slot, 5.3)) * 2.0 - 1.0) * 0.8;
			vec2 dir = normalize(vec2(hash(vec2(slot, 8.9)) - 0.5, -0.6));
			vec2 head = from + dir * age * 0.9;
			vec2 q = d.xz / (d.y + 0.35) * 0.6;
			vec2 rel = q - head;
			float along = dot(rel, -dir);
			float across = abs(dot(rel, vec2(-dir.y, dir.x)));
			float streak = smoothstep(0.004, 0.0, across) * step(0.0, along) * smoothstep(0.25, 0.0, along);
			col += vec3(1.0, 0.95, 0.85) * streak * (1.0 - age / 0.9) * (1.0 - day) * 2.5;
		}
	}
	// The sun by day, a smaller cooler moon by night.
	float sd = dot(d, normalize(sun_dir));
	float md = dot(d, -normalize(sun_dir));
	col += vec3(1.0, 0.92, 0.78) * (smoothstep(0.9993, 0.9997, sd) * 6.0 + pow(max(sd, 0.0), 180.0) * 0.6) * day;
	col += vec3(0.75, 0.82, 1.0) * smoothstep(0.9995, 0.9998, md) * 1.6 * (1.0 - day);
	// Clouds: two layers on planes above, the high one slower, both drifting with the wind and slowly changing
	// shape as they go (the noise is warped by itself over time). Denser and darker as the weather closes in.
	if (d.y > 0.0) {
		float t = clock * (0.012 + 0.05 * wind);
		vec2 base = d.xz / (d.y + 0.08);
		vec2 uv = base * 0.9 + vec2(t, t * 0.35);
		vec2 warp = vec2(fbm(uv * 0.5 + clock * 0.004), fbm(uv * 0.5 - clock * 0.003 + 4.7));
		float low = fbm(uv + warp * 0.6);
		float high = fbm(base * 0.45 + vec2(t * 0.4, -t * 0.15) + 11.0);
		float c = max(low, high * 0.92);
		float threshold = mix(0.72, 0.28, cover);
		float dens = smoothstep(threshold, threshold + 0.22, c) * smoothstep(0.0, 0.12, d.y);
		vec3 lit = mix(vec3(0.08, 0.09, 0.12), vec3(1.0), day);
		lit = mix(lit, lit * DUSK * 1.3, dusk * 0.6);
		vec3 cloud = mix(lit, tint * (0.35 + 0.45 * day), clamp(cover * 0.9 + dark * 0.4, 0.0, 1.0));
		col = mix(col, cloud, dens * 0.92);
		// Overcast: the whole dome greys over.
		col = mix(col, cloud * 0.85, cover * cover * 0.65);
	}
	col *= 1.0 - 0.55 * dark;
	col += vec3(0.75, 0.8, 1.0) * flash * 1.5;
	COLOR = col;
}
