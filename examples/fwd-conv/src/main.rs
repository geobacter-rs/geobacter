
fn convolution() {
  fn filter_<T>(input: &[T], input_stride: &[usize], input_dim: &[usize],
                input_offset: usize, input_idx_base: &[usize],
                filter: &[T], filter_stride: &[usize], filter_dim: &[usize],
                filter_offset: usize,
                padding: &[i32],
                depth: usize, depth_end: usize,
                acc: Option<T>) -> T
    where T: Add<T, Output=T> + Mul<T, Output=T> + Default + Copy,
  {
    let mut acc = acc.unwrap_or_default();

    let p = padding[0] as usize;
    let input_idx_end = input_dim[0] + 2 * p;

    let mut input_idx = input_idx_base[0];
    let mut filter_idx = 0;
    while filter_idx < filter_dim[0] {
      let i_offset = input_offset + (input_idx - p) * input_stride[0];
      let f_offset = filter_offset + filter_idx * filter_stride[0];

      let v = if input_idx < p || input_idx + 1 > input_idx_end {
        Default::default()
      } else if depth + 1 >= depth_end {
        input[i_offset] * filter[f_offset]
      } else {
        filter_(input, &input_stride[1..], &input_dim[1..],
                i_offset, &input_idx_base[1..],
                filter, &filter_stride[1..], &filter_dim[1..],
                f_offset,
                &padding[1..], depth + 1, depth_end,
                None)
      };

      acc = acc + v;

      input_idx += 1;
      filter_idx += 1;
    }

    return acc;
  }

  // depth == 0 is the first level
  fn conv<T>(input: &[T], input_stride: &[usize], input_dim: &[usize],
             top_input_offset: usize, input_offset: usize,
             input_idx_base: &mut [usize],
             filter: &[T], filter_stride: &[usize], filter_dim: &[usize],
             filter_offset: usize,
             depth: usize,
             padding: &[i32], stride: &[i32],
             output: &mut [T], output_stride: &[usize],
             output_dim: &[usize],
             output_offset: usize)
    where T: Add<T, Output=T> + Mul<T, Output=T> + Default + Copy,
  {
    let p = padding[depth] as usize;
    let input_end = input_dim[depth] + 2 * p - (filter_dim[depth]);

    let mut input_i = 0;

    let mut output_idx = 0;
    while output_idx < output_dim[0] {
      input_idx_base[depth] = input_i;
      let input_offset = input_offset + input_i * input_stride[depth];
      let output_offset = output_offset + output_idx * output_stride[0];

      if depth + 1 < input_dim.len() {
        conv(input, input_stride, input_dim, top_input_offset,
             input_offset, input_idx_base,
             filter, filter_stride, filter_dim, filter_offset,
             depth + 1,
             padding, &stride[1..], output, &output_stride[1..],
             &output_dim[1..], output_offset);
      } else {
        let v = filter_(input, input_stride, input_dim,
                        top_input_offset, &input_idx_base[..],
                        filter, filter_stride, filter_dim, filter_offset,
                        padding, 0, input_dim.len(),
                        None);
        output[output_offset] = output[output_offset] + v;
      }

      input_i += stride[0] as usize;
      output_idx += 1;
    }
  }

  fn conv_k_d1<T>(_batch: usize,
                  input: &[T], input_stride: &[usize], input_dim: &[usize],
                  input_offset: usize, input_idx_base: &mut [usize],
                  filter: &[T], filter_stride: &[usize], filter_dim: &[usize],
                  padding: &[i32], stride: &[i32],
                  output: &mut [T], output_stride: &[usize],
                  output_dim: &[usize], output_offset: usize)
    where T: Add<T, Output=T> + Mul<T, Output=T> + Default + Copy,
  {
    for k in 0..filter_dim[0] {
      let output_offset = output_offset + k * output_stride[0];
      let filter_offset = k * filter_stride[0];
      for d1 in 0..input_dim[0] {
        let input_offset = input_offset + d1 * input_stride[0];
        let filter_offset = filter_offset + d1 * filter_stride[1];

        conv(input, &input_stride[1..], &input_dim[1..],
             input_offset, input_offset, input_idx_base,
             filter, &filter_stride[2..], &filter_dim[2..], filter_offset,
             0, padding, stride, output, &output_stride[1..],
             &output_dim[1..],
             output_offset);
      }
    }
  }

  let mut input_idx = Vec::new();
  input_idx.resize(input_dim.len() - 2, 0);
  let mut output_idx = Vec::new();
  output_idx.resize(output_dim.len(), 0);

  let batches = input_dim[0];
  let mut batch = 0;
  while batch < batches {
    let input_offset = batch * input_stride[0];
    let output_offset = batch * output_stride[0];

    conv_k_d1(batch, input, &input_stride[1..], &input_dim[1..], input_offset,
              &mut input_idx[..],
              filter, &filter_stride[..], &filter_dim[..],
              &config.padding[..], &config.stride[..],
              output, &output_stride[1..], &output_dim[1..],
              output_offset);

    batch += 1;
  }
}
