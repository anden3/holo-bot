#[macro_export]
macro_rules! add_bindings {
    () => {};
    ( $i:ident: |$($a:ident: $t:ty),*| = $snd:ident $(:: $snd_path:ident)* => $evt:ty; $($rest:tt)* ) => {
        #[instrument(skip(self))]
        pub async fn $i(&self, user_id: UserId, $($a: $t),*) -> anyhow::Result<mpsc::Receiver<$evt>> {
            let (tx, rx) = mpsc::channel::<$evt>(16);

            self.update_sender
                .send($snd $(:: $snd_path)*(user_id, tx, $($a),*))
                .await
                .map_err(|e| e.into())
                .map(|_| rx)
        }

        add_bindings!($($rest)*);
    };
    ( $i:ident = $snd:ident $(:: $snd_path:ident)* => $evt:ty;  $($rest:tt)* ) => {
        #[instrument(skip(self))]
        pub async fn $i(&self, user_id: UserId) -> anyhow::Result<mpsc::Receiver<$evt>> {
            let (tx, rx) = mpsc::channel::<$evt>(16);

            self.update_sender
                .send($snd $(:: $snd_path)*(user_id, tx))
                .await
                .map_err(|e| e.into())
                .map(|_| rx)
        }

        add_bindings!($($rest)*);
    };
    ( $i:ident: |$($a:ident: $t:ty),*| = $snd:ident $(:: $snd_path:ident)*; $($rest:tt)* ) => {
        #[instrument(skip(self))]
        pub async fn $i(&self, user_id: UserId, $($a: $t),*) -> anyhow::Result<()> {
            self.update_sender
                .send($snd $(:: $snd_path)*(user_id, $($a),*))
                .await
                .map_err(|e| e.into())
        }

        add_bindings!($($rest)*);
    };
    ( $i:ident = $snd:ident $(:: $snd_path:ident)*; $($rest:tt)* ) => {
        #[instrument(skip(self))]
        pub async fn $i(&self, user_id: UserId) -> anyhow::Result<()> {
            self.update_sender
                .send($snd $(:: $snd_path)*(user_id))
                .await
                .map_err(|e| e.into())
        }

        add_bindings!($($rest)*);
    };
}

#[macro_export]
macro_rules! delegate_events {
    ( $slf:ident, $val:ident, $($i:ident: |$($a:ident),*| = $snd:ident $(:: $snd_path:ident)*),* ) => {
        match $val {
            $(
                $snd $(:: $snd_path)*(user, sender, $($a),*) => {
                    if $slf.is_user_not_in_voice_channel(user, &sender).await {
                        continue;
                    }

                    if let Err(e) = $slf.$i(&sender, $($a),*).await {
                        Self::report_error(e, &sender).await;
                    }
                }
            )*
            _ => (),
        }
    };
}

#[macro_export]
macro_rules! impl_error_variants {
    ( $($t:ty),* ) => {
        $(
            impl HasErrorVariant for $t {
                fn new_error(error: QueueError) -> Self {
                    Self::Error(error)
                }
            }
        )*
    }
}
