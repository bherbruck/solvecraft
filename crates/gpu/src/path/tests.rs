use super::*;
use solidcraft_geom::{Aabb3, Mesh};
use solidcraft_render::{Camera, ProductMesh, StandardView, surface::OpaqueSurface};

fn executor() -> Option<Executor> {
    let (adapter, device, queue) = match crate::open_headless() {
        Ok(device) => device,
        Err(e) => {
            assert!(std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "required GPU unavailable: {e}");
            eprintln!("gpu-path device test skipped: {e}");
            return None;
        }
    };
    eprintln!("gpu-path tests on {} ({:?}, {:?})", adapter.get_info().name, adapter.get_info().device_type, adapter.get_info().backend);
    Some(Executor::new(device, queue).unwrap())
}
fn request(size: u32) -> ImageRequest {
    let mut camera = Camera { distance: 4.0, ..Default::default() };
    camera.set_view(StandardView::Top);
    ImageRequest { camera, width: size, height: size, background: [10, 20, 30], transparent_background: false }
}
fn sphere(surface: OpaqueSurface) -> ProductScene {
    sphere_grid(surface, 48, 96)
}
fn sphere_grid(surface: OpaqueSurface, rows: u32, columns: u32) -> ProductScene {
    sphere_section(surface, rows, columns, rows)
}
fn sphere_section(surface: OpaqueSurface, rows: u32, columns: u32, rings: u32) -> ProductScene {
    // Original unit sphere, deliberately triangulated here rather than imported test data.
    let mut mesh = Mesh::default();
    for i in 0..=rings {
        let theta = f64::from(i) * std::f64::consts::PI / f64::from(rows);
        for j in 0..=columns {
            let phi = f64::from(j) * std::f64::consts::TAU / f64::from(columns);
            let n = Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos());
            mesh.positions.push(n);
            mesh.normals.push(n);
        }
    }
    for i in 0..rings {
        for j in 0..columns {
            let a = i * (columns + 1) + j;
            let b = a + columns + 1;
            mesh.triangles.extend([[a, b, a + 1], [a + 1, b, b + 1]]);
            mesh.tri_face.extend([0, 0]);
        }
    }
    let bounds = mesh.bounds();
    ProductScene {
        meshes: vec![ProductMesh {
            name: "original sphere".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(surface),
            transmission: None,
            faces: Vec::new(),
        }],
        bounds,
    }
}
fn white() -> OpaqueSurface {
    OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 0.0, 1.0, 1.0).unwrap()
}

fn clear_glass() -> solidcraft_render::transmission::Transmission {
    solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap()
}

#[test]
fn volume_glass_requires_closed_outward_homogeneous_geometry() {
    let mut glass = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    glass.meshes[0].transmission = Some(clear_glass());
    assert!(scene::flatten(&glass, Vec3::ZERO, &CancelToken::default()).is_ok());
    let mut open = glass.clone();
    let mesh = Arc::make_mut(&mut open.meshes[0].mesh);
    mesh.triangles.remove(300);
    mesh.tri_face.remove(300);
    assert!(scene::flatten(&open, Vec3::ZERO, &CancelToken::default()).is_err());
    let mut inward = glass;
    for t in &mut Arc::make_mut(&mut inward.meshes[0].mesh).triangles {
        t.swap(1, 2);
    }
    assert!(scene::flatten(&inward, Vec3::ZERO, &CancelToken::default()).is_err());
    open.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, Some(0.1)).unwrap());
    assert!(scene::flatten(&open, Vec3::ZERO, &CancelToken::default()).is_ok());
}

#[test]
fn nested_and_overlapping_dielectric_bounds_are_rejected_before_upload() {
    let mut outer = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    outer.meshes[0].transmission = Some(clear_glass());
    for shift in [0.0, 0.5] {
        let mut inner = outer.meshes[0].clone();
        for p in &mut Arc::make_mut(&mut inner.mesh).positions {
            *p = *p * 0.5 + Vec3::new(shift, 0.0, 0.0);
        }
        let mut scene = outer.clone();
        scene.meshes.push(inner);
        assert!(scene::flatten(&scene, Vec3::ZERO, &CancelToken::default()).is_err());
    }
}

#[test]
fn multiple_closed_shells_in_one_dielectric_body_are_rejected() {
    let mut scene = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    scene.meshes[0].transmission = Some(clear_glass());
    let second = scene.meshes[0].mesh.clone();
    let mesh = Arc::make_mut(&mut scene.meshes[0].mesh);
    let offset = mesh.positions.len() as u32;
    mesh.positions.extend(second.positions.iter().map(|p| *p + Vec3::new(4.0, 0.0, 0.0)));
    mesh.normals.extend(&second.normals);
    mesh.triangles.extend(second.triangles.iter().map(|t| t.map(|i| i + offset)));
    mesh.tri_face.extend(&second.tri_face);
    let error = scene::flatten(&scene, Vec3::ZERO, &CancelToken::default()).err().unwrap();
    assert!(error.to_string().contains("one connected shell"), "{error}");
}

#[test]
fn gpu_exact_dielectric_matches_f64_fresnel() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("dielectric_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut task = Task::new(context.clone(), sphere(white()), request(1), JobOptions::default(), CancelToken::default()).unwrap();
    for (ni, nt) in [(1.0_f64, 1.0_f64), (1.0, 1.5), (1.5, 1.0), (1.0, 3.0)] {
        for cosine in [0.0, 0.01, 0.5, (nt / ni).atan().cos(), 1.0] {
            let mut params = vec![0; 64];
            scene::vector(&mut params, Vec3::new(cosine, ni, nt), 0.0);
            params.resize(112, 0);
            context.queue.write_buffer(&task.uniform, 0, &params);
            let mut encoder = context.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &task.bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            context.queue.submit([encoder.finish()]);
            task.sample = 1;
            let actual = task.read_linear().unwrap()[0];
            let expected = solidcraft_render::transmission::fresnel(cosine, ni, nt).unwrap();
            for (a, b) in actual.iter().zip([expected.s, expected.p, expected.transmitted_cosine.unwrap_or(0.0)]) {
                assert!((f64::from(*a) - b).abs() < 2e-5, "Fresnel {ni}->{nt} cos={cosine}: {a} != {b}");
            }
        }
    }
}

#[test]
fn gpu_nonabsorbing_glass_sphere_white_furnace_conserves_radiance() {
    let Some(executor) = executor() else { return };
    let mut scene = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    scene.meshes[0].transmission = Some(clear_glass());
    scene.meshes[0].opacity = 0.2;
    let options = JobOptions { samples: 512, max_depth: 16, seed: 1948, ..Default::default() };
    let mut task = Task::new(executor.context.clone(), scene, request(16), options, CancelToken::default()).unwrap();
    task.environment = 1.0;
    task.studio = 0.0;
    for _ in 0..options.samples {
        task.dispatch().unwrap();
    }
    for (index, pixel) in task.read_linear().unwrap().iter().enumerate() {
        assert!((pixel[0] - 1.0).abs() < 0.025, "glass furnace pixel {index}: {pixel:?}");
    }
}

#[test]
fn gpu_ball_lens_focus_uses_actual_mesh_intersections() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("lens_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut scene = sphere_grid(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap(), 128, 256);
    scene.meshes[0].transmission = Some(clear_glass());
    let mut task = Task::new(context.clone(), scene, request(1), JobOptions::default(), CancelToken::default()).unwrap();
    for height in [0.01, 0.02, 0.03] {
        let mut params = vec![0; 32];
        scene::vector(&mut params, Vec3::new(height, 0.0, 3.0), 0.0);
        scene::vector(&mut params, Vec3::ZERO, 0.0);
        scene::vector(&mut params, Vec3::ZERO, 0.0);
        scene::vector(&mut params, -Vec3::Z, 1.5); // nR/(2(n-1)), measured from centre.
        scene::vector(&mut params, Vec3::new(f64::from(task.epsilon), 0.0, 0.0), 0.0);
        context.queue.write_buffer(&task.uniform, 0, &params);
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        let actual = task.read_linear().unwrap()[0];
        assert_eq!(actual[3], 1.0);
        assert!((actual[0] - 1.0).abs() < 5e-5 && (actual[1] - 1.0).abs() < 5e-5, "image-space focal spot at h={height}: {actual:?}");
    }
}

#[test]
fn gpu_camera_inside_volume_fails_without_publishing_fake_radiance() {
    let Some(executor) = executor() else { return };
    let mut scene = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    scene.meshes[0].transmission = Some(clear_glass());
    let mut request = request(1);
    request.camera.distance = 0.1;
    let mut task = Task::new(executor.context.clone(), scene, request, JobOptions::default(), CancelToken::default()).unwrap();
    task.dispatch().unwrap();
    assert_eq!(task.read_linear().unwrap_err().code, ErrorCode::InvalidScene);
}

#[test]
fn engine_preflight_keeps_cpu_alpha_and_routes_physical_glass_to_the_native_host() {
    use serde_json::json;
    let Some(executor) = executor() else { return };
    let mut s = solidcraft_engine::Session::default();
    s.set_render_job_host(Some(solidcraft_render::job::CpuHost::cooperative())).unwrap();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Glass"})).unwrap();
    s.execute("appearance.assign", &json!({"bodies":["Glass"],"color":"#c0e0ff","opacity":0.2})).unwrap();
    let cpu = s.execute("render.preflight", &json!({"settings":{"width":16,"height":16}})).unwrap();
    assert_eq!(cpu["refraction"], false);
    assert_eq!(cpu["transparency"], "sorted_alpha_approximation");
    assert!(s.execute("render.preflight", &json!({"progressive":true,"settings":{"width":16,"height":16}})).is_err());
    s.register_render_job_host(ImageHost::with_executor(Arc::new(executor)).unwrap()).unwrap();
    let snapshot = s.doc.to_json();
    let mut params = json!({"progressive":true,"settings":{"width":16,"height":16},
        "raytrace":{"dielectrics":[{"body_index":0,"body_name":"Glass","ior":1.5,"absorption_per_mm":[0.1,0.02,0],"boundary":"closed_box","thickness_mm":0}]}});
    let gpu = s.execute("render.preflight", &params).unwrap();
    assert_eq!(gpu["refraction"], true);
    assert_eq!(gpu["dielectric_boundaries"], json!(["thin_sheet", "closed_box", "closed_mesh"]));
    assert_eq!(gpu["transparency"], "physical_transmission");
    assert_eq!(gpu["resolved"]["executor"], "gpu-path");
    params["settings"]["camera"] = json!(Camera { distance: 40.0, fov: 0.7, ..Default::default() });
    params["lens"] = json!({"aperture_radius_mm":0.5,"focus_distance_mm":40});
    let lens = s.execute("render.preflight", &params).unwrap();
    assert_eq!(lens["options"]["lens"]["aperture_radius_mm"], 0.5);
    assert_eq!(snapshot, s.doc.to_json());
    params["progressive"] = json!(false);
    assert!(s.execute("render.preflight", &params).is_err());
    assert!(!solidcraft_engine::product_scene::extract(&s).unwrap().has_transmission());
}

#[test]
fn gpu_thin_lens_focus_and_defocus_match_geometric_oracle() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("camera_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for (yaw, pitch, roll) in [(0.0, 0.0, 0.0), (-0.45, 0.5, 0.7)] {
        let mut request = request(2);
        request.camera = Camera { distance: 12.0, fov: 0.7, yaw, pitch, roll, ..Default::default() };
        let camera = request.camera;
        let lens = solidcraft_render::job::ThinLens { aperture_radius_mm: 0.4, focus_distance_mm: 9.0 };
        let options = JobOptions { lens: Some(lens), ..Default::default() };
        let mut task = Task::new(context.clone(), sphere(white()), request, options, CancelToken::default()).unwrap();
        let (right, up, back) = camera.basis();
        let eye = back * camera.distance;
        for (ux, uy) in [(0.0_f64, 0.0_f64), (0.7, -0.6)] {
            let focal =
                eye + (right * ux + up * uy) * (camera.half_height() * lens.focus_distance_mm / camera.distance) - back * lens.focus_distance_mm;
            for (radial, azimuth) in [(0.0_f64, 0.0_f64), (0.25, 0.125), (0.75, 0.625), (0.999, 0.999)] {
                let mut params = task.parameters(1);
                params.truncate(96);
                scene::vector(&mut params, Vec3::new(ux, uy, radial), azimuth as f32);
                context.queue.write_buffer(&task.uniform, 0, &params);
                let mut encoder = context.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &task.bind, &[]);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                context.queue.submit([encoder.finish()]);
                task.sample = 1;
                let rays = task.read_linear().unwrap();
                let vec = |p: [f32; 4]| Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
                let origin = vec(rays[0]) - Vec3::new(16.0, 16.0, 16.0);
                let direction = vec(rays[1]) - Vec3::new(2.0, 2.0, 2.0);
                let disk = (right * (azimuth * std::f64::consts::TAU).cos() + up * (azimuth * std::f64::consts::TAU).sin())
                    * (radial.sqrt() * lens.aperture_radius_mm);
                assert!((origin - eye - disk).len() < 3e-6);
                let focus_hit = origin + direction * ((focal - origin).dot(back) / direction.dot(back));
                assert!((focus_hit - focal).len() < 5e-6, "focus {focus_hit:?} != {focal:?}");
                // At axial depth z, signed displacement from the pinhole hit is
                // disk*(1-z/focus). Check both sides, including the flip beyond focus.
                for depth in [4.5, 18.0] {
                    let centre = eye - back * depth;
                    let hit = origin + direction * ((centre - origin).dot(back) / direction.dot(back));
                    let pinhole = eye + (focal - eye) * (depth / lens.focus_distance_mm);
                    assert!((hit - pinhole - disk * (1.0 - depth / lens.focus_distance_mm)).len() < 1e-5);
                }
            }
        }
    }
}

#[test]
fn gpu_zero_aperture_is_bit_identical_and_lens_admission_is_explicit() {
    let Some(executor) = executor() else { return };
    let mut request = request(16);
    request.camera.fov = 0.7;
    let render = |lens, fov| {
        let options = JobOptions { samples: 8, lens, seed: 1948, ..Default::default() };
        let mut image = request.clone();
        image.camera.fov = fov;
        let mut task = Task::new(executor.context.clone(), sphere(white()), image, options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.dispatch().unwrap();
        }
        task.read_linear().unwrap()
    };
    for fov in [0.0, 0.7] {
        assert_eq!(render(None, fov), render(Some(solidcraft_render::job::ThinLens { aperture_radius_mm: 0.0, focus_distance_mm: 8.0 }), fov));
    }
    let lens = Some(solidcraft_render::job::ThinLens { aperture_radius_mm: 0.3, focus_distance_mm: 4.0 });
    assert_eq!(render(lens, 0.7), render(lens, 0.7));
    request.camera.fov = 0.0;
    assert!(admission(&sphere(white()), &request, JobOptions { lens, ..Default::default() }).is_err());
    request.camera.fov = 1e-100;
    assert!(admission(&sphere(white()), &request, JobOptions { lens, ..Default::default() }).is_err());
}

#[test]
#[ignore = "manual glass transport evidence; set SOLIDCRAFT_GLASS_EVIDENCE to a PNG path"]
fn glass_transport_evidence() {
    let Some(executor) = executor() else { return };
    let output = std::env::var("SOLIDCRAFT_GLASS_EVIDENCE").unwrap();
    let mut meshes = Vec::new();
    for (centre, radius, color) in
        [(Vec3::new(-1.3, 0.0, 1.5), 1.5, Vec3::new(0.75, 0.94, 1.0)), (Vec3::new(2.0, 1.0, 1.0), 1.0, Vec3::new(1.0, 0.7, 0.35))]
    {
        let mut scene = sphere_grid(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap(), 96, 192);
        let mut body = scene.meshes.remove(0);
        for p in &mut Arc::make_mut(&mut body.mesh).positions {
            *p = *p * radius + centre;
        }
        body.transmission =
            Some(solidcraft_render::transmission::Transmission::new(Vec3::new(-color.x.ln(), -color.y.ln(), -color.z.ln()) / 5.0, None).unwrap());
        meshes.push(body);
    }
    for y in -16..16 {
        for x in -16..16 {
            let color = if (x + y) % 2 == 0 { Vec3::new(0.65, 0.7, 0.75) } else { Vec3::new(0.035, 0.055, 0.08) };
            let x = f64::from(x) * 2.0;
            let y = f64::from(y) * 2.0;
            let mesh = Mesh {
                positions: vec![
                    Vec3::new(x, y, -0.01),
                    Vec3::new(x + 2.0, y, -0.01),
                    Vec3::new(x + 2.0, y + 2.0, -0.01),
                    Vec3::new(x, y + 2.0, -0.01),
                ],
                normals: vec![Vec3::Z; 4],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                tri_face: vec![0, 0],
                ..Default::default()
            };
            meshes.push(ProductMesh {
                name: "original checker tile".into(),
                mesh: Arc::new(mesh),
                color: [200; 3],
                opacity: 1.0,
                surface: Some(OpaqueSurface::new(color, 0.0, 0.65, 1.5).unwrap()),
                transmission: None,
                faces: vec![],
            });
        }
    }
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |a, m| a.union(&m.mesh.bounds()));
    let camera = Camera { target: Vec3::new(0.0, 0.0, 1.0), yaw: -0.45, pitch: 0.5, distance: 13.0, fov: 0.65, ..Default::default() };
    let request = ImageRequest { camera, width: 960, height: 720, background: [236, 240, 245], transparent_background: false };
    let lens =
        std::env::var_os("SOLIDCRAFT_GLASS_DOF").map(|_| solidcraft_render::job::ThinLens { aperture_radius_mm: 0.5, focus_distance_mm: 12.0 });
    let options = JobOptions { samples: 256, max_depth: 16, seed: 1948, lens, ..Default::default() };
    let mut task = Task::new(executor.context.clone(), ProductScene { meshes, bounds }, request, options, CancelToken::default()).unwrap();
    task.environment = 1.0;
    loop {
        if let TaskPoll::Complete(png) = task.step(4096).unwrap() {
            std::fs::write(&output, png).unwrap();
            break;
        }
    }
}

fn glass_slab(thin: bool) -> ProductScene {
    let mut mesh = Mesh::default();
    let faces = [
        ([[-10., -10., 0.5], [10., -10., 0.5], [10., 10., 0.5], [-10., 10., 0.5]], Vec3::Z),
        ([[-10., 10., -0.5], [10., 10., -0.5], [10., -10., -0.5], [-10., -10., -0.5]], -Vec3::Z),
        ([[10., -10., -0.5], [10., 10., -0.5], [10., 10., 0.5], [10., -10., 0.5]], Vec3::X),
        ([[-10., 10., -0.5], [-10., -10., -0.5], [-10., -10., 0.5], [-10., 10., 0.5]], -Vec3::X),
        ([[10., 10., -0.5], [-10., 10., -0.5], [-10., 10., 0.5], [10., 10., 0.5]], Vec3::Y),
        ([[-10., -10., -0.5], [10., -10., -0.5], [10., -10., 0.5], [-10., -10., 0.5]], -Vec3::Y),
    ];
    for (index, (positions, normal)) in faces.into_iter().enumerate() {
        if thin && index > 0 {
            break;
        }
        let offset = mesh.positions.len() as u32;
        for p in positions {
            mesh.positions.push(Vec3::new(p[0], p[1], p[2]));
            mesh.normals.push(normal);
        }
        mesh.triangles.extend([[offset, offset + 1, offset + 2], [offset, offset + 2, offset + 3]]);
        mesh.tri_face.extend([0, 0]);
    }
    let bounds = mesh.bounds();
    ProductScene {
        bounds,
        meshes: vec![ProductMesh {
            name: "original slab".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap()),
            transmission: Some(
                solidcraft_render::transmission::Transmission::new(Vec3::new(-0.5_f64.ln(), -0.8_f64.ln(), 0.0), thin.then_some(1.0)).unwrap(),
            ),
            faces: vec![],
        }],
    }
}

#[test]
fn gpu_solid_and_thin_slabs_match_absorbing_interreflection_oracle() {
    let Some(executor) = executor() else { return };
    for (thin, thickness) in [(false, 1.0), (true, 1.0), (true, 0.0)] {
        let mut scene = glass_slab(thin);
        if thin {
            let sigma = scene.meshes[0].transmission.unwrap().extinction();
            scene.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(sigma, Some(thickness)).unwrap());
        }
        let (_, expected) = solidcraft_render::transmission::slab(scene.meshes[0].transmission.unwrap(), thickness, 1.0, 1.0, 1.5).unwrap();
        let mut request = request(1);
        request.camera.fov = 0.0001;
        let options = JobOptions { samples: 4096, max_depth: 16, seed: 1948, ..Default::default() };
        let mut task = Task::new(executor.context.clone(), scene, request, options, CancelToken::default()).unwrap();
        task.environment = 1.0;
        task.environment_bottom_only = true;
        // Deliberately large offset: Beer uses unshifted geometric endpoints.
        task.epsilon = 0.05;
        task.studio = 0.0;
        for _ in 0..options.samples {
            task.dispatch().unwrap();
        }
        let actual = task.read_linear().unwrap()[0];
        for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z]) {
            assert!((f64::from(*a) - b).abs() < 0.015, "slab thin={thin} thickness={thickness}: {actual:?} vs {expected:?}");
        }
    }
}

#[test]
fn shader_validates_without_a_device() {
    let module = naga::front::wgsl::parse_str(SHADER).unwrap();
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap();
}

#[test]
fn admission_rejects_hostile_parameters_and_transparency() {
    let mut scene = sphere(white());
    let mut r = request(16);
    for size in [0, 4097, u32::MAX] {
        r.width = size;
        assert!(admission(&scene, &r, JobOptions::default()).is_err());
    }
    r.width = 16;
    for samples in [0, 4097, u32::MAX] {
        assert!(admission(&scene, &r, JobOptions { samples, ..Default::default() }).is_err());
    }
    for depth in [0, 17, u32::MAX] {
        assert!(admission(&scene, &r, JobOptions { max_depth: depth, ..Default::default() }).is_err());
    }
    for ev in [f64::NAN, f64::INFINITY, 33.0] {
        assert!(admission(&scene, &r, JobOptions { exposure_ev: ev, ..Default::default() }).is_err());
    }
    scene.meshes[0].opacity = 0.5;
    assert!(admission(&scene, &r, JobOptions::default()).is_err());
    scene.meshes[0].opacity = 1.0;
    Arc::make_mut(&mut scene.meshes[0].mesh).triangles[0][0] = u32::MAX;
    assert!(scene::flatten(&scene, Vec3::ZERO, &CancelToken::default()).is_err());
}

#[test]
fn bvh_handles_empty_and_cancelled_scenes() {
    let empty = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let upload = scene::flatten(&empty, Vec3::ZERO, &CancelToken::default()).unwrap();
    assert_eq!(upload.nodes.len(), 32);
    assert_eq!(&upload.nodes[12..16], &u32::MAX.to_le_bytes());
    let cancel = CancelToken::default();
    cancel.cancel();
    assert_eq!(scene::flatten(&sphere(white()), Vec3::ZERO, &cancel).err().unwrap().code, ErrorCode::Cancelled);
}

#[test]
fn gpu_brdf_matches_f64_surface_oracle() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("path BRDF reference"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for metallic in [0.0, 0.5, 1.0] {
        for roughness in [0.045, 0.25, 1.0] {
            for ior in [1.0, 1.5, 3.0] {
                let surface = OpaqueSurface::new(Vec3::new(0.8, 0.5, 0.2), metallic, roughness, ior).unwrap();
                let mut task = Task::new(context.clone(), sphere(surface), request(1), JobOptions::default(), CancelToken::default()).unwrap();
                for (view, light) in [
                    (Vec3::Z, Vec3::Z),
                    (Vec3::new(0.5, 0.2, 1.0), Vec3::new(-0.4, 0.3, 0.8)),
                    (Vec3::new(1.0, 0.0, 0.02), Vec3::new(-1.0, 0.1, 0.02)),
                ] {
                    let view = view.normalized().unwrap();
                    let light = light.normalized().unwrap();
                    let mut params = vec![0; 48];
                    scene::vector(&mut params, light, 0.0);
                    scene::vector(&mut params, Vec3::Z, 0.0);
                    scene::vector(&mut params, view, 0.0);
                    scene::vector(&mut params, Vec3::ZERO, 0.0);
                    context.queue.write_buffer(&task.uniform, 0, &params);
                    let mut encoder = context.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &task.bind, &[]);
                        pass.dispatch_workgroups(1, 1, 1);
                    }
                    context.queue.submit([encoder.finish()]);
                    task.sample = 1;
                    let actual = task.read_linear().unwrap()[0];
                    let expected = surface.brdf(Vec3::Z, view, light).unwrap();
                    for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z]) {
                        assert!((f64::from(*a) - b).abs() <= 3e-3 * b.abs() + 2e-5, "m={metallic}, r={roughness}, ior={ior}: {a} vs {b}");
                    }
                }
            }
        }
    }
}

#[test]
fn gpu_tiny_scene_matches_cpu_studio_with_face_appearance() {
    let Some(executor) = executor() else { return };
    let surface = OpaqueSurface::new(Vec3::new(0.7, 0.2, 0.1), 0.5, 0.3, 1.5).unwrap();
    let mesh = Mesh {
        positions: vec![Vec3::new(-3.0, -3.0, 0.0), Vec3::new(3.0, -3.0, 0.0), Vec3::new(3.0, 3.0, 0.0), Vec3::new(-3.0, 3.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![7, 9],
        ..Default::default()
    };
    let bounds = mesh.bounds();
    let scene = ProductScene {
        meshes: vec![ProductMesh {
            name: "reference plane".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(white()),
            transmission: None,
            faces: [7, 9]
                .map(|face| solidcraft_render::FaceAppearance { face, color: [0; 3], opacity: 1.0, surface: Some(surface), transmission: None })
                .to_vec(),
        }],
        bounds,
    };
    let mut task =
        Task::new(executor.context.clone(), scene, request(4), JobOptions { samples: 4, max_depth: 1, ..Default::default() }, CancelToken::default())
            .unwrap();
    for _ in 0..4 {
        task.dispatch().unwrap();
    }
    let actual = task.read_linear().unwrap();
    let expected = solidcraft_render::studio::shade(surface, Vec3::Z, Vec3::Z).unwrap();
    for pixel in actual {
        assert_eq!(pixel[3], 1.0);
        for (a, b) in pixel.iter().zip([expected.x, expected.y, expected.z]) {
            assert!((f64::from(*a) - b).abs() < 3e-3 * b.abs() + 2e-5, "{a} vs {b}");
        }
    }
}

#[test]
fn gpu_furnace_fixed_seed_and_cancellation() {
    let Some(executor) = executor() else { return };
    let options = JobOptions { samples: 4096, max_depth: 8, seed: 37, ..Default::default() };
    let mut task = Task::new(executor.context.clone(), sphere(white()), request(8), options, CancelToken::default()).unwrap();
    task.environment = 1.0;
    task.studio = 0.0;
    for _ in 0..options.samples {
        task.dispatch().unwrap();
    }
    let result = task.read_linear().unwrap();
    eprintln!("furnace centre {:?}; BVH {:.3} ms; upload {} bytes / {:.3} ms", result[27], task.metrics.0, task.metrics.1, task.metrics.2);
    // The oracle includes Schlick entry/exit attenuation even at IOR=1, plus single-
    // scattering GGX. It is near-white, not an exactly energy-preserving Lambert model.
    for index in [27, 28, 35, 36] {
        assert!((result[index][0] - 1.0).abs() < 0.07, "furnace radiance: {:?}", result[index]);
    }
    let mut repeat = Task::new(executor.context.clone(), sphere(white()), request(8), options, CancelToken::default()).unwrap();
    repeat.environment = 1.0;
    repeat.studio = 0.0;
    for _ in 0..options.samples {
        repeat.dispatch().unwrap();
    }
    assert_eq!(result, repeat.read_linear().unwrap(), "fixed sample indices and seed must repeat on the same adapter");
    let token = CancelToken::default();
    let mut cancelled = Task::new(executor.context.clone(), sphere(white()), request(16), options, token.clone()).unwrap();
    cancelled.dispatch().unwrap();
    token.cancel();
    assert_eq!(cancelled.step(4096).err().unwrap().code, ErrorCode::Cancelled);
}

#[test]
fn skinny_polar_triangles_do_not_shadow_themselves() {
    let Some(executor) = executor() else { return };
    let mut image = request(32);
    image.camera.distance = 16.0;
    image.camera.fov = 0.0005;
    let mut task = Task::new(
        executor.context.clone(),
        sphere_section(white(), 512, 1024, 1),
        image,
        JobOptions { samples: 1, max_depth: 1, ..Default::default() },
        CancelToken::default(),
    )
    .unwrap();
    task.dispatch().unwrap();
    let pixels = task.read_linear().unwrap();
    for (index, pixel) in pixels.iter().enumerate() {
        assert_eq!(pixel[3], 1.0, "polar cap covers the image: {index} {pixel:?}");
        assert!(pixel[0] > 0.1, "outward-facing cap must receive unobstructed studio light: {index} {pixel:?}");
    }
}

#[test]
fn native_host_cancel_never_publishes_a_frame() {
    let Some(executor) = executor() else { return };
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let job = host
        .submit_with_options(
            solidcraft_render::job::JobId { session: 1, serial: 1, generation: 1 },
            1,
            sphere(white()),
            request(64),
            JobOptions { samples: 4096, ..Default::default() },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = job.status().unwrap();
        if status.progress.phase == Phase::Running {
            break;
        }
        assert!(!status.progress.phase.terminal(), "{status:?}");
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    job.cancel().unwrap();
    assert_eq!(job.wait_timeout(Duration::from_secs(30)).unwrap().progress.phase, Phase::Cancelled);
    assert!(job.frame().unwrap().is_none());
}

/// Explicit benchmark: large allocation and sustained GPU load must not run in ordinary CI.
#[test]
#[ignore = "manual 1M-triangle 1920x1080 GPU performance measurement"]
fn path_performance() {
    let executor = executor().expect("benchmark requires an adapter");
    let samples = std::env::var("SOLIDCRAFT_PATH_BENCH_SAMPLES").ok().map(|s| s.parse::<u32>().unwrap()).unwrap_or(1);
    let options = JobOptions { samples, ..Default::default() };
    options.validate().unwrap();
    let surface = OpaqueSurface::new(Vec3::new(0.8, 0.4, 0.2), 0.25, 0.4, 1.5).unwrap();
    let scene = sphere_grid(surface, 512, 1024);
    let mut request = request(1920);
    request.height = 1080;
    let admitted = executor.admit(&scene, &request, options).unwrap();
    let mut task = Task::new(executor.context.clone(), scene, request, options, CancelToken::default()).unwrap();
    let started = Instant::now();
    while task.sample < samples {
        task.dispatch().unwrap();
    }
    let seconds = started.elapsed().as_secs_f64();
    let linear = task.read_linear().unwrap();
    assert!(linear.iter().all(|p| p.iter().all(|v| v.is_finite())));
    let coverage = linear.iter().map(|p| f64::from(p[3])).sum::<f64>() / linear.len() as f64;
    assert!((0.16..0.19).contains(&coverage), "sphere must occupy the expected projected area: {coverage}");
    let centre = linear[540 * 1920 + 960];
    let lit = linear.iter().filter(|p| p[0] > 0.01).count();
    eprintln!(
        "PATH_BENCH_DIAGNOSTIC coverage={coverage:.6} centre={centre:?} lit_pixels={lit} nearby={:?}",
        [linear[500 * 1920 + 960], linear[540 * 1920 + 1000], linear[580 * 1920 + 960]]
    );
    assert_eq!(centre[3], 1.0);
    assert!(centre[0] > 0.01, "benchmark must actually shade the sphere: {centre:?}");
    eprintln!("PATH_BENCH_IMAGE coverage={coverage:.6} centre={centre:?}");
    eprintln!(
        "PATH_BENCH input_triangles={} resolution=1920x1080 spp={} bvh_ms={:.3} upload_bytes={} upload_ms={:.3} admitted_bytes={} render_seconds={:.6} ms_per_spp={:.3}",
        admitted.triangles,
        samples,
        task.metrics.0,
        task.metrics.1,
        task.metrics.2,
        admitted.estimated_total_bytes,
        seconds,
        seconds * 1000.0 / f64::from(samples)
    );
}
