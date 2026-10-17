// Original software BVH and unidirectional Monte Carlo integrator. All directions
// at a surface point away from it, matching render::surface's GGX/Smith oracle.
struct Triangle {
    p0: vec4<f32>, p1: vec4<f32>, p2: vec4<f32>,
    n0: vec4<f32>, n1: vec4<f32>, n2: vec4<f32>,
    // base.w: dielectric flag, parameters.w: thin-sheet thickness (-1 = solid).
    // optics.xyz: extinction per mm, optics.w: body/medium identity (1..16384).
    base: vec4<f32>, parameters: vec4<f32>, optics: vec4<f32>,
};
struct Node { lo: vec3<f32>, first: u32, hi: vec3<f32>, count: u32 };
struct AreaLight { center: vec4<f32>, u: vec4<f32>, v: vec4<f32>, radiance: vec4<f32> };
struct Params {
    // width, height, sample index, pixel offset
    image: vec4<u32>,
    // seed, maximum depth, tile length, reserved
    sampling: vec4<u32>,
    eye: vec4<f32>, right: vec4<f32>, up: vec4<f32>, back: vec4<f32>,
    // ray epsilon, test environment radiance, studio scale, reserved
    lighting: vec4<f32>,
    environment: vec4<f32>, // RGB radiance, enabled flag
    light_count: vec4<f32>, // w: rectangle count
    area_lights: array<AreaLight,4>,
};
@group(0) @binding(0) var<uniform> u: Params;
@group(0) @binding(1) var<storage, read> geometry: array<u32>;
@group(0) @binding(2) var<storage, read> nodes: array<Node>;
@group(0) @binding(3) var<storage, read_write> accumulation: array<vec4<f32>>;
const PI: f32 = 3.141592653589793;
const FAR: f32 = 1e30;

// Header: triangle count, vertex offset (words), material offset (words), vertex count.
// Indexed triangles contain three vertex indices and one resolved material ID.
fn vector_at(word: u32) -> vec4<f32> {
    return bitcast<vec4<f32>>(vec4<u32>(geometry[word],geometry[word+1u],geometry[word+2u],geometry[word+3u]));
}
fn triangle(index: u32) -> Triangle {
    let word = 4u+index*4u;
    let a = geometry[1]+geometry[word]*8u;
    let b = geometry[1]+geometry[word+1u]*8u;
    let c = geometry[1]+geometry[word+2u]*8u;
    let material = geometry[2]+geometry[word+3u]*12u;
    return Triangle(vector_at(a),vector_at(b),vector_at(c),vector_at(a+4u),vector_at(b+4u),vector_at(c+4u),vector_at(material),vector_at(material+4u),vector_at(material+8u));
}

// Exact s/p power reflectance and transmitted cosine; -1 denotes TIR.
fn dielectric(c: f32, ni: f32, nt: f32) -> vec3<f32> {
    if ni == nt { return vec3<f32>(0.0,0.0,c); }
    let eta = ni/nt;
    let s2 = eta*eta*max(0.0,1.0-c*c);
    if s2 >= 1.0 { return vec3<f32>(1.0,1.0,-1.0); }
    let ct = sqrt(1.0-s2);
    let rs = (ni*c-nt*ct)/(ni*c+nt*ct);
    let rp = (nt*c-ni*ct)/(nt*c+ni*ct);
    return vec3<f32>(rs*rs,rp*rp,ct);
}
struct Sheet { reflection: vec3<f32>, transmission: vec3<f32> };
fn sheet_polarization(r: f32, a: vec3<f32>) -> Sheet {
    let denominator = vec3<f32>(1.0)-r*r*a*a;
    // The grazing lossless limit is perfectly reflecting.
    if r >= 1.0 { return Sheet(vec3<f32>(1.0),vec3<f32>(0.0)); }
    return Sheet(vec3<f32>(r)+(1.0-r)*(1.0-r)*r*a*a/denominator,(1.0-r)*(1.0-r)*a/denominator);
}
fn sheet(t: Triangle, f: vec3<f32>) -> Sheet {
    if f.z <= 0.0 { return Sheet(vec3<f32>(1.0),vec3<f32>(0.0)); }
    let attenuation = exp(-t.optics.xyz*(t.parameters.w/f.z));
    let s = sheet_polarization(f.x,attenuation);
    let p = sheet_polarization(f.y,attenuation);
    return Sheet((s.reflection+p.reflection)*0.5,(s.transmission+p.transmission)*0.5);
}
struct Medium { id: f32, ior: f32, sigma: vec3<f32> };

fn random(state: ptr<function, u32>) -> f32 {
    *state = *state * 747796405u + 2891336453u;
    let word = ((*state >> ((*state >> 28u) + 4u)) ^ *state) * 277803737u;
    return f32(((word >> 22u) ^ word) >> 8u) * (1.0 / 16777216.0);
}

fn unit(v: vec3<f32>) -> vec3<f32> {
    let scale = max(max(abs(v.x), abs(v.y)), abs(v.z));
    if scale == 0.0 { return vec3<f32>(0.0, 0.0, 1.0); }
    return normalize(v / scale);
}
fn fresnel(f0: vec3<f32>, c: f32) -> vec3<f32> {
    let t = 1.0 - clamp(c, 0.0, 1.0);
    return f0 + (vec3<f32>(1.0) - f0) * (t*t*t*t*t);
}
fn distribution(nh: f32, a2: f32) -> f32 {
    let d = (1.0 - nh*nh) + nh*nh*a2;
    return a2 / (PI*d*d);
}
fn brdf(t: Triangle, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let nv = clamp(dot(n,v), -1.0, 1.0);
    let nl = clamp(dot(n,l), -1.0, 1.0);
    if nv <= 0.0 || nl <= 0.0 { return vec3<f32>(0.0); }
    let h = unit(v+l);
    let a = t.parameters.y*t.parameters.y;
    let a2 = a*a;
    let d = distribution(clamp(dot(n,h), 0.0, 1.0), a2);
    let gv = 2.0 / (nv + sqrt(a2 + (1.0-a2)*nv*nv));
    let gl = 2.0 / (nl + sqrt(a2 + (1.0-a2)*nl*nl));
    let ratio = (t.parameters.z - 1.0) / (t.parameters.z + 1.0);
    let f0 = mix(vec3<f32>(ratio*ratio), t.base.xyz, t.parameters.x);
    let diffuse = t.base.xyz * ((1.0-t.parameters.x)/PI);
    return fresnel(f0, dot(v,h)) * (d*0.25*gv*gl)
        + (vec3<f32>(1.0)-fresnel(f0,nv)) * (vec3<f32>(1.0)-fresnel(f0,nl)) * diffuse;
}

// Explicit parallel slab handling avoids 0*infinity/NaN at box boundaries.
fn box_hit(node: Node, o: vec3<f32>, d: vec3<f32>, limit: f32) -> bool {
    var near = 0.0;
    var far = limit;
    for (var k = 0u; k < 3u; k += 1u) {
        if abs(d[k]) < 1e-20 {
            if o[k] < node.lo[k]-u.lighting.x || o[k] > node.hi[k]+u.lighting.x { return false; }
        } else {
            let a = (node.lo[k]-u.lighting.x-o[k])/d[k];
            let b = (node.hi[k]+u.lighting.x-o[k])/d[k];
            near = max(near, min(a,b)); far = min(far, max(a,b));
        }
    }
    return far >= near;
}
struct Hit { distance: f32, triangle: u32, uv: vec2<f32> };
fn trace(o: vec3<f32>, d: vec3<f32>, any_hit: bool) -> Hit {
    var hit = Hit(FAR, 0xffffffffu, vec2<f32>(0.0));
    // Project every candidate into the same ray-aligned 2D coordinate system.
    // Shared edges then use identical products with opposite signs, avoiding the
    // independently rounded barycentric comparisons of Moller-Trumbore.
    var kz = 0u;
    if abs(d.y)>abs(d.x) { kz=1u; }
    if abs(d.z)>abs(d[kz]) { kz=2u; }
    let kx=(kz+1u)%3u; let ky=(kx+1u)%3u;
    let shear=vec3<f32>(d[kx]/d[kz],d[ky]/d[kz],1.0/d[kz]);
    // A balanced tree over <= 2^20 triangles needs <= 21 pending nodes.
    var stack: array<u32, 32>;
    stack[0] = 0u;
    var size = 1u;
    loop {
        if size == 0u { break; }
        size -= 1u;
        let node = nodes[stack[size]];
        if node.first == 0xffffffffu || !box_hit(node, o, d, hit.distance) { continue; }
        if node.count == 0u {
            if size + 2u > 32u { break; }
            stack[size] = node.first; stack[size+1u] = node.first+1u; size += 2u;
            continue;
        }
        for (var i = node.first; i < node.first + node.count; i += 1u) {
            let t = triangle(i);
            let pa=t.p0.xyz-o; let pb=t.p1.xyz-o; let pc=t.p2.xyz-o;
            let a=vec2<f32>(pa[kx]-shear.x*pa[kz],pa[ky]-shear.y*pa[kz]);
            let b=vec2<f32>(pb[kx]-shear.x*pb[kz],pb[ky]-shear.y*pb[kz]);
            let c=vec2<f32>(pc[kx]-shear.x*pc[kz],pc[ky]-shear.y*pc[kz]);
            let weights=vec3<f32>(b.x*c.y-b.y*c.x,c.x*a.y-c.y*a.x,a.x*b.y-a.y*b.x);
            if any(weights<vec3<f32>(0.0)) && any(weights>vec3<f32>(0.0)) { continue; }
            let det=weights.x+weights.y+weights.z;
            if det==0.0 { continue; }
            let distance=dot(weights,vec3<f32>(pa[kz],pb[kz],pc[kz])*shear.z)/det;
            if distance > u.lighting.x && distance < hit.distance {
                hit = Hit(distance,i,weights.yz/det);
                if any_hit { return hit; }
            }
        }
    }
    return hit;
}

fn from_local(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let axis = select(vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,1.0,0.0),abs(n.z)>0.9);
    let x = unit(cross(axis,n));
    return unit(x*p.x + cross(n,x)*p.y + n*p.z);
}
fn sample_surface(t: Triangle, n: vec3<f32>, v: vec3<f32>, rng: ptr<function,u32>) -> vec3<f32> {
    let component = random(rng);
    let x = random(rng); let phi = 2.0*PI*random(rng);
    if component < 0.5 {
        let radius = sqrt(x);
        return from_local(vec3<f32>(radius*cos(phi),radius*sin(phi),sqrt(1.0-x)),n);
    }
    let alpha = t.parameters.y*t.parameters.y;
    let cosine = sqrt((1.0-x)/(1.0+(alpha*alpha-1.0)*x));
    let sine = sqrt(max(0.0,1.0-cosine*cosine));
    let h = from_local(vec3<f32>(sine*cos(phi),sine*sin(phi),cosine),n);
    return reflect(-v,h);
}
// Conditional opaque mixture density; branch selection is accounted for by callers.
fn surface_pdf(t: Triangle, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>) -> f32 {
    let nl = dot(n,l);
    if nl <= 0.0 { return 0.0; }
    let h = unit(v+l);
    let a = t.parameters.y*t.parameters.y;
    let nh = max(dot(n,h),0.0);
    return 0.5*nl/PI + 0.5*distribution(nh,a*a)*nh/(4.0*max(dot(v,h),1e-20));
}
fn power_weight(a: f32, b: f32) -> f32 {
    // Ratio form avoids overflow for grazing/very distant rectangular samples.
    if a <= 0.0 { return 0.0; }
    let ratio = b/a;
    return 1.0/(1.0+ratio*ratio);
}
struct EmitterHit { distance: f32, index: u32 };
fn emitter_hit(o: vec3<f32>, d: vec3<f32>) -> EmitterHit {
    var hit = EmitterHit(FAR,0xffffffffu);
    for (var i=0u; i<u32(u.light_count.w); i+=1u) {
        let light = u.area_lights[i];
        let normal = unit(cross(light.u.xyz,light.v.xyz));
        let cosine = dot(normal,d);
        if abs(cosine)<1e-8 { continue; }
        let distance = dot(light.center.xyz-o,normal)/cosine;
        if distance <= u.lighting.x || distance >= hit.distance { continue; }
        let q = o+d*distance-light.center.xyz;
        if abs(dot(q,light.u.xyz))<=dot(light.u.xyz,light.u.xyz) && abs(dot(q,light.v.xyz))<=dot(light.v.xyz,light.v.xyz) {
            hit = EmitterHit(distance,i);
        }
    }
    return hit;
}
fn area_pdf(light: AreaLight, delta: vec3<f32>) -> f32 {
    let cosine = dot(unit(cross(light.u.xyz,light.v.xyz)),-unit(delta));
    if cosine<=0.0 { return 0.0; }
    return dot(delta,delta)/(light.center.w*cosine);
}
fn visible(o: vec3<f32>, d: vec3<f32>, distance: f32) -> bool {
    // Closest hit is essential: an arbitrary first BVH leaf may lie behind a light.
    return trace(o,d,false).distance >= distance && emitter_hit(o,d).distance >= distance;
}
fn direct_studio(t: Triangle, position: vec3<f32>, o: vec3<f32>, n: vec3<f32>, geometric: vec3<f32>, v: vec3<f32>, last: bool, rng: ptr<function,u32>) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    let opaque = 1.0-t.base.w;
    if opaque<=0.0 { return result; }
    for (var i=0u; i<u32(u.light_count.w); i+=1u) {
        let light = u.area_lights[i];
        let point = light.center.xyz + light.u.xyz*(2.0*random(rng)-1.0) + light.v.xyz*(2.0*random(rng)-1.0);
        let delta = point-position;
        let direction = unit(delta);
        let pdf = area_pdf(light,delta);
        let nl = dot(n,direction);
        if pdf<=0.0 || nl<=0.0 || dot(geometric,direction)<=0.0 { continue; }
        let shadow = point-o;
        if !visible(o,unit(shadow),length(shadow)-u.lighting.x*2.0) { continue; }
        let weight = select(power_weight(pdf,opaque*surface_pdf(t,n,v,direction)),1.0,last);
        result += brdf(t,n,v,direction)*light.radiance.xyz*(opaque*nl*weight/pdf);
    }
    if any(u.environment.xyz>vec3<f32>(0.0)) {
        // Uniform sphere density is the same for NEE and BSDF escape evaluation.
        let z = 1.0-2.0*random(rng);
        let phi = 2.0*PI*random(rng);
        let radius = sqrt(max(0.0,1.0-z*z));
        let direction = vec3<f32>(radius*cos(phi),radius*sin(phi),z);
        let nl = dot(n,direction);
        let pdf = 1.0/(4.0*PI);
        if nl>0.0 && dot(geometric,direction)>0.0 && visible(o,direction,FAR) {
            let weight = select(power_weight(pdf,opaque*surface_pdf(t,n,v,direction)),1.0,last);
            result += brdf(t,n,v,direction)*u.environment.xyz*(opaque*nl*weight/pdf);
        }
    }
    return result;
}
struct CameraRay { origin: vec3<f32>, direction: vec3<f32> };
// The lens lies in the eye plane. A uniform-area disk (r=sqrt(u), phi=2pi*v)
// aims at the pinhole ray's intersection with the axial focus plane.
fn primary_ray(uv: vec2<f32>, lens_sample: vec2<f32>) -> CameraRay {
    let offset = u.right.xyz*uv.x + u.up.xyz*uv.y;
    var o = u.eye.xyz + offset;
    var d = -u.back.xyz;
    if u.eye.w > 0.5 { o = u.eye.xyz; d = unit(offset-u.back.xyz*u.back.w); }
    if u.eye.w > 0.5 && u.right.w > 0.0 {
        let radius = sqrt(lens_sample.x)*u.right.w;
        let phi = 2.0*PI*lens_sample.y;
        let displacement = radius*(cos(phi)*unit(u.right.xyz)+sin(phi)*unit(u.up.xyz));
        // Construct the focal point directly, avoiding division by a tiny ray cosine.
        let focal = offset*(u.up.w/u.back.w)-u.back.xyz*u.up.w;
        o += displacement;
        d = unit(focal-displacement);
    }
    return CameraRay(o,d);
}
fn sample_path(pixel: u32) -> vec4<f32> {
    var rng = pixel ^ (u.image.z * 0x9e3779b9u) ^ u.sampling.x;
    let xy = vec2<f32>(f32(pixel % u.image.x)+random(&rng), f32(pixel / u.image.x)+random(&rng));
    let uv = vec2<f32>(xy.x/f32(u.image.x)*2.0-1.0, 1.0-xy.y/f32(u.image.y)*2.0);
    var lens_sample = vec2<f32>(0.0);
    // Zero aperture consumes no variates: preserve the pinhole sample stream exactly.
    if u.eye.w > 0.5 && u.right.w > 0.0 { lens_sample = vec2<f32>(random(&rng),random(&rng)); }
    let primary = primary_ray(uv,lens_sample);
    var o = primary.origin;
    var d = primary.direction;
    var previous_position = o;
    var throughput = vec3<f32>(1.0);
    var radiance = vec3<f32>(0.0);
    var coverage = 0.0;
    var previous_pdf = 0.0;
    var previous_delta = true;
    var media: array<Medium,16>;
    var medium_count = 0u;
    let lights = array<vec4<f32>,3>(vec4<f32>(0.6,0.0,0.8,3.0),vec4<f32>(-0.8,0.0,0.6,1.0),vec4<f32>(0.0,-0.6,0.8,1.5));
    for (var bounce = 0u; bounce < u.sampling.y; bounce += 1u) {
        let hit = trace(o,d,false);
        if u.environment.w > 0.0 {
            let emitter = emitter_hit(o,d);
            if emitter.distance < hit.distance {
                let light = u.area_lights[emitter.index];
                let point = o+d*emitter.distance;
                if medium_count>0u {
                    throughput *= exp(-media[medium_count-1u].sigma*length(point-previous_position));
                }
                let pdf = area_pdf(light,point-previous_position);
                if pdf>0.0 {
                    let weight = select(power_weight(previous_pdf,pdf),1.0,previous_delta);
                    radiance += throughput*light.radiance.xyz*weight;
                }
                coverage = 1.0;
                break;
            }
        }
        if hit.triangle == 0xffffffffu {
            // Camera-inside and intersecting/open volumes are deliberately unsupported.
            if medium_count > 0u { return vec4<f32>(f32(bounce),0.0,0.0,-1.0); }
            if u.environment.w > 0.0 {
                // The backplate remains request.background; environment lights covered paths.
                if bounce>0u {
                    let weight = select(power_weight(previous_pdf,1.0/(4.0*PI)),1.0,previous_delta);
                    radiance += throughput*u.environment.xyz*weight;
                }
            } else if u.sampling.w == 0u || d.z < 0.0 { radiance += throughput * u.lighting.y; }
            break;
        }
        coverage = 1.0;
        let t = triangle(hit.triangle); let v = -d;
        var geometric = unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz));
        let entering = dot(geometric,v)>=0.0;
        if !entering { geometric = -geometric; }
        var ni = 1.0;
        if medium_count > 0u {
            let current = media[medium_count-1u];
            ni = current.ior;
        }
        var n = unit(t.n0.xyz*(1.0-hit.uv.x-hit.uv.y)+t.n1.xyz*hit.uv.x+t.n2.xyz*hit.uv.y);
        if dot(n,geometric)<0.0 { n = -n; }
        if dot(n,v)<=0.0 { n = geometric; }
        // Recover the surface point from barycentrics. A distant ray's f32 distance
        // loses precision on skinny triangles; o+d*t can land behind the surface
        // and make the next shadow ray intersect the same face.
        let position = t.p0.xyz*(1.0-hit.uv.x-hit.uv.y) + t.p1.xyz*hit.uv.x + t.p2.xyz*hit.uv.y;
        if medium_count > 0u {
            throughput *= exp(-media[medium_count-1u].sigma*length(position-previous_position));
        }
        previous_position = position;
        o = position + geometric*(u.lighting.x*2.0);
        if u.environment.w > 0.0 {
            radiance += throughput*direct_studio(t,position,o,n,geometric,v,bounce+1u>=u.sampling.y,&rng);
        }
        for (var i=0u; i<3u && u.environment.w==0.0; i+=1u) {
            let light = lights[i];
            if u.lighting.z > 0.0 && dot(geometric,light.xyz)>0.0 && dot(n,light.xyz)>0.0 {
                if trace(o,light.xyz,true).triangle == 0xffffffffu {
                    radiance += throughput * brdf(t,n,v,light.xyz) * ((1.0-t.base.w)*light.w*dot(n,light.xyz)*u.lighting.z);
                }
            }
        }
        if t.base.w > 0.0 && random(&rng) < t.base.w {
            previous_delta = true;
            var nt = t.parameters.z;
            if t.parameters.w < 0.0 && entering && medium_count > 0u {
                return vec4<f32>(f32(bounce),0.0,0.0,-4.0);
            }
            if t.parameters.w < 0.0 && !entering {
                if medium_count == 0u { return vec4<f32>(f32(bounce),0.0,0.0,-2.0); }
                if media[medium_count-1u].id != t.optics.w { return vec4<f32>(f32(bounce),0.0,0.0,-3.0); }
                nt = 1.0;
                if medium_count > 1u { nt = media[medium_count-2u].ior; }
            }
            var f = dielectric(clamp(dot(n,v),0.0,1.0),ni,nt);
            var transmitted = refract(d,n,ni/nt);
            // A bent shading normal must not send a transmitted ray into the wrong
            // geometric hemisphere. Fall back to the geometric interface there.
            if (f.z >= 0.0 && dot(transmitted,geometric) >= 0.0) || dot(reflect(d,n),geometric) <= 0.0 {
                n = geometric;
                f = dielectric(clamp(dot(n,v),0.0,1.0),ni,nt);
                transmitted = refract(d,n,ni/nt);
            }
            if t.parameters.w >= 0.0 {
                let weights = sheet(t,f);
                let mean_r = dot(weights.reflection,vec3<f32>(1.0/3.0));
                let mean_t = dot(weights.transmission,vec3<f32>(1.0/3.0));
                if mean_r+mean_t <= 0.0 { break; }
                let probability = mean_r/(mean_r+mean_t);
                if random(&rng) < probability {
                    throughput *= weights.reflection/probability;
                    d = reflect(d,n);
                } else {
                    throughput *= weights.transmission/(1.0-probability);
                    o = position-geometric*(u.lighting.x*2.0);
                    // Parallel thin sheet: unchanged direction, no optical displacement.
                }
            } else if random(&rng) < (f.x+f.y)*0.5 {
                d = reflect(d,n);
            } else {
                if entering {
                    if medium_count >= 16u { return vec4<f32>(f32(bounce),0.0,0.0,-4.0); }
                    media[medium_count] = Medium(t.optics.w,nt,t.optics.xyz);
                    medium_count += 1u;
                } else { medium_count -= 1u; }
                throughput *= (ni/nt)*(ni/nt);
                d = unit(transmitted);
                o = position-geometric*(u.lighting.x*2.0);
            }
            if dot(d,geometric) > 0.0 { o = position+geometric*(u.lighting.x*2.0); }
            continue;
        }
        d = sample_surface(t,n,v,&rng);
        let nl = dot(n,d);
        if nl<=0.0 || dot(geometric,d)<=0.0 { break; }
        let pdf = surface_pdf(t,n,v,d);
        previous_pdf = (1.0-t.base.w)*pdf;
        previous_delta = false;
        throughput *= brdf(t,n,v,d) * (nl/pdf);
        if bounce>=3u {
            let survive = clamp(max(max(throughput.x,throughput.y),throughput.z),0.05,0.95);
            if random(&rng)>survive { break; }
            throughput /= survive;
        }
    }
    return vec4<f32>(radiance,coverage);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let pixel = u.image.w + gid.x;
    if gid.x >= u.sampling.z || pixel >= u.image.x*u.image.y { return; }
    if u.image.z > 0u && accumulation[pixel].w < 0.0 { return; }
    let sample = sample_path(pixel);
    if sample.w < 0.0 { accumulation[pixel] = vec4<f32>(sample.x,f32(u.image.z),f32(pixel),sample.w); return; }
    if u.image.z == 0u { accumulation[pixel] = sample; }
    else { accumulation[pixel] += sample; }
}

// Validation entry point: exact same BRDF used by production paths, read back in tests.
@compute @workgroup_size(1)
fn reference() {
    accumulation[0] = vec4<f32>(brdf(triangle(0u),u.up.xyz,u.back.xyz,u.right.xyz),1.0);
}

@compute @workgroup_size(1)
fn dielectric_reference() {
    let f = dielectric(u.up.x,u.up.y,u.up.z);
    accumulation[0] = vec4<f32>(f.xy,max(f.z,0.0),1.0);
}

@compute @workgroup_size(1)
fn camera_reference() {
    let ray = primary_ray(u.lighting.xy,u.lighting.zw);
    // Bias signed test coordinates for the radiance readback's nonnegative contract.
    accumulation[0] = vec4<f32>(ray.origin+vec3<f32>(16.0),1.0);
    accumulation[1] = vec4<f32>(ray.direction+vec3<f32>(2.0),1.0);
}

// Deterministic image-space ball-lens check. Uses the production BVH/intersection
// and interpolated normal data, with Snell transport through both interfaces.
@compute @workgroup_size(1)
fn lens_reference() {
    let direction = u.back.xyz;
    let entry = trace(u.eye.xyz,direction,false);
    if entry.triangle == 0xffffffffu { accumulation[0] = vec4<f32>(0.0); return; }
    let a = triangle(entry.triangle);
    let pa = a.p0.xyz*(1.0-entry.uv.x-entry.uv.y)+a.p1.xyz*entry.uv.x+a.p2.xyz*entry.uv.y;
    let na = unit(a.n0.xyz*(1.0-entry.uv.x-entry.uv.y)+a.n1.xyz*entry.uv.x+a.n2.xyz*entry.uv.y);
    let inside = refract(direction,na,1.0/a.parameters.z);
    let exit = trace(pa-na*u.lighting.x*2.0,inside,false);
    if exit.triangle == 0xffffffffu { accumulation[0] = vec4<f32>(0.0); return; }
    let b = triangle(exit.triangle);
    let pb = b.p0.xyz*(1.0-exit.uv.x-exit.uv.y)+b.p1.xyz*exit.uv.x+b.p2.xyz*exit.uv.y;
    let nb = unit(b.n0.xyz*(1.0-exit.uv.x-exit.uv.y)+b.n1.xyz*exit.uv.x+b.n2.xyz*exit.uv.y);
    let outgoing = refract(inside,-nb,b.parameters.z);
    let spot = pb+outgoing*((-u.back.w-pb.z)/outgoing.z);
    accumulation[0] = vec4<f32>(spot+vec3<f32>(1.0,1.0,3.0),1.0);
}
