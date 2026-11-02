//! Present code for the GPU compositor.

use super::*;

impl GpuRenderer {
  pub fn present(&mut self) -> Result<(), String> {
    if self.reconfigure {
      self.surface.configure(&self.device, &self.config);
      self.reconfigure = false;
    }
    if !self.ui_uploaded {
      return Ok(());
    }
    self
      .queue
      .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&self.globals));

    let Some(frame) = self.acquire()? else {
      return Ok(());
    };
    let view = frame
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("dawn-composite"),
      });
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("dawn-composite-pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      pass.set_pipeline(&self.pipeline);
      pass.set_bind_group(0, &self.bind_group, &[]);
      pass.draw(0..3, 0..1);
    }
    self.queue.submit(Some(encoder.finish()));
    self.queue.present(frame);
    Ok(())
  }

  /// Acquires one swapchain texture, recovering once from a stale surface.
  /// Timeout and occlusion are normal on a compositor and simply skip a frame.
  pub(super) fn acquire(&mut self) -> Result<Option<wgpu::SurfaceTexture>, String> {
    match self.surface.get_current_texture() {
      wgpu::CurrentSurfaceTexture::Success(frame) => Ok(Some(frame)),
      wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
        self.reconfigure = true;
        Ok(Some(frame))
      }
      wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => Ok(None),
      wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
        // The compositor resized or recreated the surface. Reconfigure and
        // retry once instead of tearing down a working login form.
        self.surface.configure(&self.device, &self.config);
        self.reconfigure = false;
        match self.surface.get_current_texture() {
          wgpu::CurrentSurfaceTexture::Success(frame)
          | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(Some(frame)),
          wgpu::CurrentSurfaceTexture::Validation => {
            Err("GPU surface validation failed after reconfigure".to_string())
          }
          wgpu::CurrentSurfaceTexture::Timeout
          | wgpu::CurrentSurfaceTexture::Occluded
          | wgpu::CurrentSurfaceTexture::Outdated
          | wgpu::CurrentSurfaceTexture::Lost => Ok(None),
        }
      }
      wgpu::CurrentSurfaceTexture::Validation => Err("GPU surface validation failed".to_string()),
    }
  }

  pub(super) fn upload(&self, texture: &wgpu::Texture, pixmap: &Pixmap) {
    let size = (pixmap.width(), pixmap.height());
    self.queue.write_texture(
      wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      pixmap.data(),
      wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(size.0 * 4),
        rows_per_image: Some(size.1),
      },
      wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
      },
    );
  }

  pub(super) fn ensure_background_size(&mut self, size: (u32, u32)) {
    if self.background.size == size {
      return;
    }
    self.background = TextureSlot::new(&self.device, size, "dawn-background");
    self.bg_key = None;
    self.rebuild_bind_group();
  }

  pub(super) fn ensure_previous_size(&mut self, size: (u32, u32)) {
    if self.previous.size == size {
      return;
    }
    self.previous = TextureSlot::new(&self.device, size, "dawn-previous");
    self.prev_key = None;
    self.rebuild_bind_group();
  }

  pub(super) fn rebuild_bind_group(&mut self) {
    self.bind_group = make_bind_group(
      &self.device,
      &self.bind_group_layout,
      &self.sampler,
      &self.globals_buffer,
      &self.background,
      &self.previous,
      &self.ui,
      &self.decor,
    );
  }
}
