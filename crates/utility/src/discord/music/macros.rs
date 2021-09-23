#[macro_export]
macro_rules! add_bindings {
    () => {};
    ( $i:ident: |$($a:ident: $t:ty),*| = $snd:ident $(:: $snd_path:ident)* => $evt:ty; $($rest:tt)* ) => {
        #[instrument(skip(self))]
        pub async fn $i(&self, $($a: $t)*) -> anyhow::Result<mpsc::Receiver<$evt>> {
            let (tx, rx) = mpsc::channel::<$evt>(16);

            self.update_sender
                .send($snd $(:: $snd_path)*(tx, $($a),*))
                .await
                .map_err(|e| e.into())
                .map(|_| rx)
        }

        add_bindings!($($rest)*);
    };
    ( $i:ident = $snd:ident $(:: $snd_path:ident)* => $evt:ty;  $($rest:tt)* ) => {
        #[instrument(skip(self))]
        pub async fn $i(&self) -> anyhow::Result<mpsc::Receiver<$evt>> {
            let (tx, rx) = mpsc::channel::<$evt>(16);

            self.update_sender
                .send($snd $(:: $snd_path)*(tx))
                .await
                .map_err(|e| e.into())
                .map(|_| rx)
        }

        add_bindings!($($rest)*);
    };
    ( $i:ident: |$($a:ident: $t:ty),*| = $snd:ident $(:: $snd_path:ident)*; $($rest:tt)* ) => {
        #[instrument(skip(self))]
        pub async fn $i(&self, $($a: $t)*) -> anyhow::Result<()> {
            self.update_sender
                .send($snd $(:: $snd_path)*($($a),*))
                .await
                .map_err(|e| e.into())
        }

        add_bindings!($($rest)*);
    };
    ( $i:ident = $snd:ident $(:: $snd_path:ident)*; $($rest:tt)* ) => {
        #[instrument(skip(self))]
        pub async fn $i(&self) -> anyhow::Result<()> {
            self.update_sender
                .send($snd $(:: $snd_path)*)
                .await
                .map_err(|e| e.into())
        }

        add_bindings!($($rest)*);
    };
}

#[macro_export]
macro_rules! impl_error_variants {
    ( $($t:ty),* ) => {
        $(
            impl HasErrorVariant for $t {
                fn new_error(error: String) -> Self {
                    Self::Error(error)
                }
            }
        )*
    }
}
