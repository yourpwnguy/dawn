//! Blur code for the GPU compositor.

use super::*;

impl GpuRenderer {
  pub fn blur(&mut self, source: &Pixmap, blur: &BlurConfig, scale: f32) -> Result<Pixmap, String> {
    let result = self.blur_inner(source, blur, scale);
    match &result {
      Ok(_) => self.blur_failed_logged = false,
      Err(error) => {
        if !self.blur_failed_logged {
          warn!(%error, "GPU backdrop blur failed; using the CPU blur");
          self.blur_failed_logged = true;
        }
      }
    }
    result
  }

  pub(super) fn blur_inner(
    &mut self,
    source: &Pixmap,
    blur: &BlurConfig,
    scale: f32,
  ) -> Result<Pixmap, String> {
    let size = (source.width(), source.height());
    if size.0 == 0 || size.1 == 0 {
      return Err("cannot blur an empty pixmap".to_string());
    }
    if size.0 > self.max_dimension || size.1 > self.max_dimension {
      return Err(format!(
        "blur source {}x{} exceeds the GPU texture limit of {}",
        size.0, size.1, self.max_dimension
      ));
    }
    if self
      .blur_targets
      .as_ref()
      .is_none_or(|targets| targets.size != size)
    {
      self.create_blur_targets(size)?;
    }
    let started = Instant::now();

    // Upload the raw source. Every colour operation after this point runs in
    // the shader, so the CPU never touches the 1080p buffer.
    let Some(targets) = self.blur_targets.as_ref() else {
      return Err("blur targets disappeared after creation".to_string());
    };
    self.upload(&targets.input.texture, source);

    let radius = (blur.size.max(0.0) * scale).max(0.0);
    let contrast = blur.contrast.max(0.0);
    let color = [
      blur.vibrancy,
      blur.vibrancy_darkness,
      blur.noise,
      blur.brightness,
    ];
    let passes = blur.passes as f32;
    self.queue.write_buffer(
      &targets.horizontal_params,
      0,
      bytemuck::bytes_of(&BlurParams {
        dir_radius: [1.0, 0.0, radius, contrast],
        color,
        flags: [0.0, passes, 0.0, 0.0],
      }),
    );
    self.queue.write_buffer(
      &targets.vertical_params,
      0,
      bytemuck::bytes_of(&BlurParams {
        dir_radius: [0.0, 1.0, radius, contrast],
        color,
        flags: [1.0, passes, 0.0, 0.0],
      }),
    );

    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("dawn-blur"),
      });
    // Horizontal pass: input -> temp.
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("dawn-blur-horizontal"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &targets.temp.view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      pass.set_pipeline(&self.blur_pipeline);
      pass.set_bind_group(0, &targets.horizontal, &[]);
      pass.draw(0..3, 0..1);
    }
    // Vertical pass: temp -> output.
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("dawn-blur-vertical"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &targets.output.view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      pass.set_pipeline(&self.blur_pipeline);
      pass.set_bind_group(0, &targets.vertical, &[]);
      pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
      wgpu::TexelCopyTextureInfo {
        texture: &targets.output.texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      wgpu::TexelCopyBufferInfo {
        buffer: &targets.staging,
        layout: wgpu::TexelCopyBufferLayout {
          offset: 0,
          bytes_per_row: Some(targets.padded_row),
          rows_per_image: Some(size.1),
        },
      },
      wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
      },
    );
    self.queue.submit(Some(encoder.finish()));

    targets
      .staging
      .slice(..)
      .map_async(wgpu::MapMode::Read, |_| {});
    if let Err(error) = self.device.poll(wgpu::PollType::Wait {
      submission_index: None,
      timeout: None,
    }) {
      // Cancel the pending map so the next attempt starts from a clean buffer.
      targets.staging.unmap();
      return Err(format!("GPU blur readback poll failed: {error}"));
    }

    let readback = (|| -> Result<Pixmap, String> {
      let mapped = targets
        .staging
        .slice(..)
        .get_mapped_range()
        .map_err(|error| format!("GPU blur readback did not map: {error}"))?;
      let mut output = Pixmap::new(size.0, size.1)
        .ok_or_else(|| "cannot allocate the blur output pixmap".to_string())?;
      let row_bytes = (size.0 * 4) as usize;
      let padded = targets.padded_row as usize;
      for y in 0..size.1 as usize {
        let source_start = y * padded;
        let source_row = &mapped[source_start..source_start + row_bytes];
        let target_start = y * row_bytes;
        output.data_mut()[target_start..target_start + row_bytes].copy_from_slice(source_row);
      }
      Ok(output)
    })();
    // The buffer must be unmapped before it can be a copy destination again,
    // whether or not the row copy above succeeded.
    targets.staging.unmap();
    let output = readback?;

    debug!(
      elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
      width = size.0,
      height = size.1,
      "GPU backdrop blur"
    );
    Ok(output)
  }

  pub(super) fn create_blur_targets(&mut self, size: (u32, u32)) -> Result<(), String> {
    let input = TextureSlot::with_usage(
      &self.device,
      size,
      "dawn-blur-input",
      wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    let temp = TextureSlot::with_usage(
      &self.device,
      size,
      "dawn-blur-temp",
      wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let output = TextureSlot::with_usage(
      &self.device,
      size,
      "dawn-blur-output",
      wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_SRC,
    );
    let padded_row = padded_row(size.0);
    let staging_size = u64::from(padded_row) * u64::from(size.1);
    let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("dawn-blur-readback"),
      size: staging_size,
      usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let uniform = |label: &str| {
      self.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: std::mem::size_of::<BlurParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
      })
    };
    let horizontal_params = uniform("dawn-blur-horizontal-params");
    let vertical_params = uniform("dawn-blur-vertical-params");
    let horizontal = make_blur_bind_group(
      &self.device,
      &self.blur_layout,
      &self.blur_sampler,
      &input,
      &horizontal_params,
    );
    let vertical = make_blur_bind_group(
      &self.device,
      &self.blur_layout,
      &self.blur_sampler,
      &temp,
      &vertical_params,
    );
    self.blur_targets = Some(BlurTargets {
      size,
      input,
      temp,
      output,
      staging,
      padded_row,
      horizontal_params,
      vertical_params,
      horizontal,
      vertical,
    });
    Ok(())
  }
}
